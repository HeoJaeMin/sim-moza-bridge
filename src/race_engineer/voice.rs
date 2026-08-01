const MAX_TRANSIENT_RADIO_AGE_MS: u128 = 5_000;
const MAX_PERSISTENT_RADIO_AGE_MS: u128 = 20_000;
const MAX_CRITICAL_RADIO_AGE_MS: u128 = 60_000;
const DUPLICATE_RADIO_COOLDOWN_MS: u128 = 60_000;

#[cfg(windows)]
struct VoiceSpeaker {
    mailbox: Arc<VoiceMailbox>,
}

#[derive(Clone, Debug)]
struct VoiceJob {
    queued_at_unix_ms: u128,
    source: String,
    session_uid: Option<u64>,
    timeline_revision: u64,
    state_revision: u64,
    session_type: Option<u8>,
    lap: Option<u8>,
    position: Option<u8>,
    priority: CallPriority,
    kind: &'static str,
    message: String,
}

#[derive(Debug)]
struct LastSpokenRadio {
    spoken_at_unix_ms: u128,
    message: String,
}

#[derive(Serialize)]
struct SpokenRadioRecord<'a> {
    schema_version: u8,
    queued_at_unix_ms: u128,
    spoken_at_unix_ms: u128,
    source: &'a str,
    session_uid: Option<u64>,
    timeline_revision: u64,
    state_revision: u64,
    session_type: Option<u8>,
    lap: Option<u8>,
    position: Option<u8>,
    priority: CallPriority,
    kind: &'static str,
    message: &'a str,
}

#[derive(Default)]
struct PendingVoiceJobs {
    scope: RadioScope,
    scope_initialized: bool,
    pending: Vec<VoiceJob>,
    closed: bool,
}

impl PendingVoiceJobs {
    fn synchronize(&mut self, scope: RadioScope) {
        let timeline_changed = !self.scope_initialized
            || self.scope.source_key != scope.source_key
            || self.scope.session_uid != scope.session_uid
            || self.scope.timeline_revision != scope.timeline_revision;
        if timeline_changed {
            self.pending.clear();
        }
        self.scope = scope;
        self.scope_initialized = true;
        let current_scope = self.scope;
        self.pending
            .retain(|job| voice_job_matches_scope(job, current_scope));
    }

    fn enqueue(&mut self, job: VoiceJob) -> bool {
        if self.closed || !self.scope_initialized || !self.is_current(&job) {
            return false;
        }
        let key = voice_coalesce_key(job.kind);
        if let Some(index) = self
            .pending
            .iter()
            .position(|pending| voice_coalesce_key(pending.kind) == key)
        {
            self.pending[index] = job;
        } else {
            self.pending.push(job);
        }
        true
    }

    fn take_next(&mut self) -> Option<VoiceJob> {
        let index = self
            .pending
            .iter()
            .enumerate()
            .filter(|(_, job)| job.priority == CallPriority::Critical || self.scope.safe_to_speak)
            .min_by_key(|(_, job)| {
                (
                    voice_priority_rank(job.priority),
                    radio_kind_rank(job.kind),
                    job.queued_at_unix_ms,
                )
            })
            .map(|(index, _)| index)?;
        Some(self.pending.remove(index))
    }

    fn is_current(&self, job: &VoiceJob) -> bool {
        self.scope_initialized && voice_job_matches_scope(job, self.scope)
    }
}

fn voice_job_matches_scope(job: &VoiceJob, scope: RadioScope) -> bool {
    stable_source_key(&job.source) == scope.source_key
        && job.session_uid == scope.session_uid
        && job.timeline_revision == scope.timeline_revision
        && job.state_revision
            == scope
                .state_revisions
                .revision_for(voice_state_key(job.kind))
}

fn stable_source_key(source: &str) -> u64 {
    source
        .bytes()
        .fold(1_469_598_103_934_665_603, |hash, byte| {
            (hash ^ byte as u64).wrapping_mul(1_099_511_628_211)
        })
}

fn voice_priority_rank(priority: CallPriority) -> u8 {
    match priority {
        CallPriority::Critical => 0,
        CallPriority::Important => 1,
        CallPriority::Normal => 2,
    }
}

#[cfg(windows)]
struct VoiceMailbox {
    state: Mutex<PendingVoiceJobs>,
    available: Condvar,
}

#[cfg(windows)]
impl VoiceMailbox {
    fn new() -> Self {
        Self {
            state: Mutex::new(PendingVoiceJobs::default()),
            available: Condvar::new(),
        }
    }

    fn synchronize(&self, scope: RadioScope) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.synchronize(scope);
        self.available.notify_all();
    }

    fn enqueue(&self, job: VoiceJob) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.enqueue(job) {
            self.available.notify_one();
        }
    }

    fn wait_next(&self) -> Option<VoiceJob> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if state.closed {
                return None;
            }
            if let Some(job) = state.take_next() {
                return Some(job);
            }
            state = self
                .available
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn has_higher_priority_than(&self, priority: CallPriority) -> bool {
        let rank = voice_priority_rank(priority);
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending
            .iter()
            .any(|job| voice_priority_rank(job.priority) < rank)
    }

    fn is_current(&self, job: &VoiceJob) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_current(job)
    }

    fn close(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.closed = true;
        state.pending.clear();
        self.available.notify_all();
    }
}

#[cfg(windows)]
enum VoicePlayback {
    Spoken,
    Preempted,
}

#[cfg(windows)]
fn play_windows_voice(job: &VoiceJob, mailbox: &VoiceMailbox) -> Result<VoicePlayback, String> {
    let script = concat!(
        "Add-Type -AssemblyName System.Speech; ",
        "$utf8 = New-Object System.Text.UTF8Encoding($false); ",
        "$reader = [System.IO.StreamReader]::new([Console]::OpenStandardInput(), $utf8); ",
        "$text = $reader.ReadToEnd(); ",
        "$speaker = New-Object System.Speech.Synthesis.SpeechSynthesizer; ",
        "$ko = $speaker.GetInstalledVoices() | ",
        "Where-Object { $_.Enabled -and $_.VoiceInfo.Culture.Name -eq 'ko-KR' } | ",
        "Select-Object -First 1; ",
        "if ($null -ne $ko) { $speaker.SelectVoice($ko.VoiceInfo.Name) }; ",
        "$speaker.Rate = 1; $speaker.Volume = 100; ",
        "if ($text.Length -gt 0) { $speaker.Speak($text) }"
    );
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.creation_flags(0x0800_0000);
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start voice process: {error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "voice input pipe is unavailable".to_owned())?;
    stdin
        .write_all(job.message.replace(['\r', '\n'], " ").as_bytes())
        .map_err(|error| format!("could not write voice input: {error}"))?;
    drop(stdin);

    let timeout = Duration::from_secs((8 + (job.message.chars().count() as u64 / 8)).clamp(12, 30));
    let started = Instant::now();
    loop {
        if mailbox.has_higher_priority_than(job.priority) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(VoicePlayback::Preempted);
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(VoicePlayback::Spoken),
            Ok(Some(status)) => return Err(format!("voice process exited with {status}")),
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "voice process exceeded {} seconds",
                    timeout.as_secs()
                ));
            }
            Err(error) => return Err(format!("voice process status failed: {error}")),
        }
    }
}

#[cfg(windows)]
impl VoiceSpeaker {
    fn start(radio_path: Option<&Path>, radio_hook: Option<HookWorker>) -> Option<Self> {
        let mut radio_log = radio_path.and_then(|path| {
            let path_text = path.to_string_lossy();
            match open_jsonl(&path_text, "race engineer spoken radio") {
                Ok(log) => Some(log),
                Err(error) => {
                    eprintln!("[engineer-warning] {error}; spoken radio log disabled");
                    None
                }
            }
        });
        let mailbox = Arc::new(VoiceMailbox::new());
        let worker_mailbox = Arc::clone(&mailbox);
        thread::Builder::new()
            .name("race-engineer-voice".to_owned())
            .spawn(move || {
                let mut last_spoken =
                    HashMap::<(u64, Option<u64>, u64, &'static str), LastSpokenRadio>::new();
                while let Some(job) = worker_mailbox.wait_next() {
                    let now = unix_ms();
                    if !worker_mailbox.is_current(&job)
                        || voice_job_is_stale(&job, now)
                        || should_suppress_voice_job(&job, now, &last_spoken)
                    {
                        continue;
                    }
                    if !worker_mailbox.is_current(&job) {
                        continue;
                    }
                    match play_windows_voice(&job, &worker_mailbox) {
                        Ok(VoicePlayback::Spoken) => {}
                        Ok(VoicePlayback::Preempted) => continue,
                        Err(error) => {
                            eprintln!("[engineer-warning] Windows TTS playback failed: {error}");
                            continue;
                        }
                    }
                    last_spoken.insert(
                        voice_spoken_key(&job),
                        LastSpokenRadio {
                            spoken_at_unix_ms: unix_ms(),
                            message: job.message.clone(),
                        },
                    );
                    let Some(log) = &mut radio_log else {
                        continue;
                    };
                    let record = SpokenRadioRecord {
                        schema_version: 2,
                        queued_at_unix_ms: job.queued_at_unix_ms,
                        spoken_at_unix_ms: unix_ms(),
                        source: &job.source,
                        session_uid: job.session_uid,
                        timeline_revision: job.timeline_revision,
                        state_revision: job.state_revision,
                        session_type: job.session_type,
                        lap: job.lap,
                        position: job.position,
                        priority: job.priority,
                        kind: job.kind,
                        message: &job.message,
                    };
                    if serde_json::to_writer(&mut *log, &record)
                        .and_then(|()| log.write_all(b"\n").map_err(serde_json::Error::io))
                        .and_then(|()| log.flush().map_err(serde_json::Error::io))
                        .is_err()
                    {
                        eprintln!(
                            "[engineer-warning] failed to write spoken radio log; disabling log"
                        );
                        radio_log = None;
                    } else if let Some(hook) = &radio_hook {
                        hook.trigger();
                    }
                }
            })
            .map_err(|error| {
                eprintln!("[engineer-warning] failed to create voice worker: {error}");
                error
            })
            .ok()?;
        Some(Self { mailbox })
    }

    fn say(&self, job: VoiceJob) {
        self.mailbox.enqueue(job);
    }

    fn synchronize(&self, scope: RadioScope) {
        self.mailbox.synchronize(scope);
    }
}

#[cfg(windows)]
impl Drop for VoiceSpeaker {
    fn drop(&mut self) {
        self.mailbox.close();
    }
}

fn voice_coalesce_key(kind: &str) -> &str {
    match kind {
        "yellow_flag" | "red_flag" | "green_flag" => "track_flag",
        "safety_car" | "virtual_safety_car" | "race_restart" => "race_control",
        "rain_started" | "track_drying" | "weather_forecast" => "weather",
        "pit_window_open" | "pit_window_latest" | "strategy_stay_out" => "pit_strategy",
        _ => kind,
    }
}

fn voice_state_key(kind: &str) -> &str {
    match kind {
        "yellow_flag" | "red_flag" | "green_flag" => "track_flag",
        "safety_car" | "virtual_safety_car" | "race_restart" => "race_control",
        "rain_started" | "track_drying" | "weather_forecast" => "conditions",
        "pit_limiter" => "pit",
        "tyre_wear" | "tyre_damage" | "front_wing_damage" | "rear_wing_damage"
        | "engine_damage" | "gearbox_damage" => "damage",
        "rival_pit_front" | "rival_pit_behind" | "rival_pit_safety_car" => "rival",
        "pit_window_open" | "pit_window_latest" | "strategy_stay_out" | "strategy_reassess"
        | "fuel_target" | "ers_target" | "tyre_degradation" => "strategy",
        _ => kind,
    }
}

fn radio_kind_rank(kind: &str) -> u8 {
    match kind {
        "red_flag" | "safety_car" | "virtual_safety_car" | "yellow_flag" | "tyre_damage"
        | "front_wing_damage" | "rear_wing_damage" | "engine_damage" | "gearbox_damage" => 0,
        "race_restart" | "strategy_stay_out" | "rival_pit_front" | "rival_pit_behind"
        | "pit_window_latest" | "pit_window_open" | "fuel_target" | "ers_target"
        | "tyre_degradation" | "rain_started" | "weather_forecast" => 1,
        "front_gap" | "behind_gap" => 2,
        _ => 3,
    }
}

fn voice_job_is_stale(job: &VoiceJob, now_unix_ms: u128) -> bool {
    let max_age = if job.priority == CallPriority::Critical {
        MAX_CRITICAL_RADIO_AGE_MS
    } else if matches!(
        job.kind,
        "practice_program"
            | "session_finished"
            | "tyre_damage"
            | "front_wing_damage"
            | "rear_wing_damage"
            | "engine_damage"
            | "gearbox_damage"
    ) {
        MAX_PERSISTENT_RADIO_AGE_MS
    } else {
        MAX_TRANSIENT_RADIO_AGE_MS
    };
    now_unix_ms.saturating_sub(job.queued_at_unix_ms) > max_age
}

fn voice_spoken_key(job: &VoiceJob) -> (u64, Option<u64>, u64, &'static str) {
    (
        stable_source_key(&job.source),
        job.session_uid,
        job.timeline_revision,
        voice_coalesce_key(job.kind),
    )
}

fn should_suppress_voice_job(
    job: &VoiceJob,
    now_unix_ms: u128,
    last_spoken: &HashMap<(u64, Option<u64>, u64, &'static str), LastSpokenRadio>,
) -> bool {
    let spoken_key = voice_spoken_key(job);
    let Some(previous) = last_spoken.get(&spoken_key) else {
        return false;
    };
    let elapsed = now_unix_ms.saturating_sub(previous.spoken_at_unix_ms);
    if previous.message == job.message && elapsed < DUPLICATE_RADIO_COOLDOWN_MS {
        return true;
    }
    let kind_cooldown_ms = match spoken_key.3 {
        "position" => 12_000,
        "front_gap" | "behind_gap" => 20_000,
        _ => 0,
    };
    elapsed < kind_cooldown_ms
}

#[cfg(not(windows))]
struct VoiceSpeaker;

#[cfg(not(windows))]
impl VoiceSpeaker {
    fn start(_radio_path: Option<&Path>, _radio_hook: Option<HookWorker>) -> Option<Self> {
        eprintln!(
            "[engineer-warning] voice output is currently available only on Windows; console calls remain enabled"
        );
        None
    }

    fn say(&self, _job: VoiceJob) {}

    fn synchronize(&self, _scope: RadioScope) {}
}
