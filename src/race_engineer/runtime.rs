#[derive(Clone, Copy, Debug)]
struct SampleCursor {
    session_time: f32,
    frame_identifier: u32,
    overall_frame_identifier: Option<u32>,
}

fn sample_is_stale(
    cursor: Option<SampleCursor>,
    session_time: f32,
    frame_identifier: u32,
    overall_frame_identifier: Option<u32>,
) -> bool {
    cursor.is_some_and(|previous| {
        if let (Some(previous), Some(current)) =
            (previous.overall_frame_identifier, overall_frame_identifier)
        {
            return current <= previous;
        }
        session_time.is_finite()
            && ((session_time < previous.session_time
                && frame_identifier <= previous.frame_identifier)
                || (session_time == previous.session_time
                    && frame_identifier < previous.frame_identifier))
    })
}

fn accept_sample(
    cursor: &mut Option<SampleCursor>,
    session_time: f32,
    frame_identifier: u32,
    overall_frame_identifier: Option<u32>,
) -> bool {
    if !session_time.is_finite()
        || sample_is_stale(
            *cursor,
            session_time,
            frame_identifier,
            overall_frame_identifier,
        )
    {
        return false;
    }
    *cursor = Some(SampleCursor {
        session_time,
        frame_identifier,
        overall_frame_identifier,
    });
    true
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallPriority {
    Normal,
    Important,
    Critical,
}

impl CallPriority {
    fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Important => "important",
            Self::Critical => "critical",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EngineerCall {
    pub priority: CallPriority,
    pub kind: &'static str,
    pub message: String,
}

impl EngineerCall {
    fn normal(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            priority: CallPriority::Normal,
            kind,
            message: message.into(),
        }
    }

    fn important(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            priority: CallPriority::Important,
            kind,
            message: message.into(),
        }
    }

    fn critical(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            priority: CallPriority::Critical,
            kind,
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
struct LoggedCall<'a> {
    schema_version: u8,
    timestamp_unix_ms: u128,
    source: &'a str,
    session_uid: Option<u64>,
    session_time: Option<f32>,
    timeline_revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeline_reset: Option<&'a TimelineReset>,
    priority: CallPriority,
    kind: &'a str,
    message: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
struct TimelineReset {
    revision: u64,
    session_uid: Option<u64>,
    rollback_from_session_time: f32,
    rollback_to_session_time: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SessionLifecycleStatus {
    Idle,
    Active,
    Finished,
    DidNotFinish,
    Disqualified,
    NotClassified,
    Ended,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct SessionLifecycle {
    status: SessionLifecycleStatus,
    ended_at_unix_ms: Option<u128>,
    end_reason: Option<&'static str>,
}

impl Default for SessionLifecycle {
    fn default() -> Self {
        Self {
            status: SessionLifecycleStatus::Idle,
            ended_at_unix_ms: None,
            end_reason: None,
        }
    }
}

impl SessionLifecycle {
    fn active() -> Self {
        Self {
            status: SessionLifecycleStatus::Active,
            ..Self::default()
        }
    }

    fn from_final_classification(classification: &FinalClassificationSample) -> Self {
        let status = match classification.result_status {
            3 => SessionLifecycleStatus::Finished,
            4 | 7 => SessionLifecycleStatus::DidNotFinish,
            5 => SessionLifecycleStatus::Disqualified,
            6 => SessionLifecycleStatus::NotClassified,
            _ => SessionLifecycleStatus::Ended,
        };
        Self {
            status,
            ended_at_unix_ms: Some(unix_ms()),
            end_reason: Some("final_classification"),
        }
    }

    fn is_terminal(self) -> bool {
        !matches!(
            self.status,
            SessionLifecycleStatus::Idle | SessionLifecycleStatus::Active
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ProgramAnnouncementKey {
    session_uid: Option<u64>,
    phase: String,
    objective: String,
    action: String,
}

impl ProgramAnnouncementKey {
    fn from_program(session_uid: Option<u64>, program: &PracticeProgram) -> Self {
        Self {
            session_uid,
            phase: program.phase.to_owned(),
            objective: program.objective.clone(),
            action: program
                .instructions
                .first()
                .cloned()
                .unwrap_or_else(|| "조건을 유지해 주행".to_owned()),
        }
    }
}

pub struct RaceEngineer {
    source: String,
    core: EngineerCore,
    log: Option<BufWriter<std::fs::File>>,
    state_path: Option<PathBuf>,
    history: Option<BufWriter<std::fs::File>>,
    history_path: Option<PathBuf>,
    input_log_path: Option<PathBuf>,
    corner_log_path: Option<PathBuf>,
    analysis_report_path: Option<PathBuf>,
    radio_path: Option<PathBuf>,
    practice_state_path: Option<PathBuf>,
    trigger_path: Option<PathBuf>,
    trigger_sequence: u64,
    last_state_write: Instant,
    last_history_write: Instant,
    ai_managed: bool,
    voice: Option<VoiceSpeaker>,
    hook: Option<HookWorker>,
    practice: PracticeAdvisor,
    race: RaceStrategyAdvisor,
    last_announced_program: Option<ProgramAnnouncementKey>,
    lifecycle: SessionLifecycle,
    resume_session_uid: Option<u64>,
}

impl RaceEngineer {
    pub fn open(config: &BridgeConfig) -> Result<Option<Self>, String> {
        if !config.race_engineer {
            return Ok(None);
        }

        let state_path = config.engineer_state.as_deref().map(PathBuf::from);
        let resume_session_uid = state_path.as_deref().and_then(load_live_state_session_uid);
        let log = config.engineer_log.as_deref().map(open_log).transpose()?;
        let history = config
            .engineer_history
            .as_deref()
            .map(|path| open_jsonl(path, "race engineer state history"))
            .transpose()?;
        let history_path = config.engineer_history.as_deref().map(PathBuf::from);
        let radio_path = config
            .engineer_state
            .as_deref()
            .or(config.engineer_log.as_deref())
            .or(config.engineer_trigger.as_deref())
            .map(|path| {
                Path::new(path)
                    .with_file_name("engineer-radio.jsonl")
                    .to_path_buf()
            })
            .or_else(|| {
                (config.engineer_voice || config.engineer_radio_hook.is_some())
                    .then(|| PathBuf::from("engineer-radio.jsonl"))
            });
        let practice_state_path = config.engineer_state.as_deref().map(|path| {
            Path::new(path)
                .with_file_name("practice-advisor.json")
                .to_path_buf()
        });
        let practice = practice_state_path
            .as_deref()
            .and_then(load_practice_advisor)
            .unwrap_or_default();
        let last_announced_program = practice.last_announced_program.clone();
        let ai_managed = config.engineer_ai_hook.is_some();
        let radio_hook = if ai_managed {
            None
        } else {
            config.engineer_radio_hook.as_deref().map(|path| {
                HookWorker::start(
                    PathBuf::from(path),
                    radio_path
                        .clone()
                        .expect("voice or a radio hook supplies a spoken radio path"),
                    "SIM_MOZA_ENGINEER_RADIO",
                    None,
                    Duration::from_secs(120),
                )
            })
        };
        let voice = if config.engineer_voice && !ai_managed {
            VoiceSpeaker::start(radio_path.as_deref(), radio_hook)
        } else {
            None
        };
        let trigger_path = config.engineer_trigger.as_deref().map(PathBuf::from);
        let hook = if let Some(path) = config.engineer_ai_hook.as_deref() {
            Some(HookWorker::start(
                PathBuf::from(path),
                trigger_path
                    .clone()
                    .expect("config supplies a default trigger path for an AI hook"),
                "SIM_MOZA_ENGINEER_AI_TRIGGER",
                config
                    .engineer_ai_task_id
                    .as_ref()
                    .map(|task_id| HookEnvironment {
                        name: "SIM_MOZA_ENGINEER_TASK_ID",
                        value: task_id.clone(),
                    }),
                Duration::from_secs(45),
            ))
        } else {
            config.engineer_hook.as_deref().map(|path| {
                HookWorker::start(
                    PathBuf::from(path),
                    trigger_path
                        .clone()
                        .expect("config supplies a default trigger path for a hook"),
                    "SIM_MOZA_ENGINEER_TRIGGER",
                    None,
                    Duration::from_secs(30),
                )
            })
        };

        Ok(Some(Self {
            source: String::new(),
            core: EngineerCore::default(),
            log,
            state_path,
            history,
            history_path,
            input_log_path: config.input_log.as_deref().map(PathBuf::from),
            corner_log_path: config.corner_log.as_deref().map(PathBuf::from),
            analysis_report_path: config.analysis_report.as_deref().map(PathBuf::from),
            radio_path,
            practice_state_path,
            trigger_path,
            trigger_sequence: 0,
            last_state_write: Instant::now() - Duration::from_secs(1),
            last_history_write: Instant::now() - Duration::from_secs(1),
            ai_managed,
            voice,
            hook,
            practice,
            race: RaceStrategyAdvisor::default(),
            last_announced_program,
            lifecycle: SessionLifecycle::default(),
            resume_session_uid,
        }))
    }

    pub fn ingest(
        &mut self,
        source: &str,
        update: &TelemetryUpdate,
        completed_lap: Option<&CompletedLapAnalysis>,
    ) {
        let source_changed = !self.source.is_empty() && self.source != source;
        let session_changed = self
            .core
            .snapshot
            .session_uid
            .zip(update.session_uid)
            .is_some_and(|(previous, current)| previous != current);
        if source_changed || session_changed {
            self.finish_session_internal(if source_changed {
                "source_replaced"
            } else {
                "session_replaced"
            });
            self.lifecycle = SessionLifecycle::active();
        } else if self.lifecycle.status == SessionLifecycleStatus::Idle && !update.is_empty() {
            self.lifecycle = SessionLifecycle::active();
        }

        if self.source.is_empty() {
            self.source.push_str(source);
            self.core = EngineerCore::default();
        } else if self.source != source {
            self.source.clear();
            self.source.push_str(source);
            self.core = EngineerCore::default();
            self.practice = PracticeAdvisor::default();
            self.race = RaceStrategyAdvisor::default();
            self.last_announced_program = None;
            self.write_practice_state();
        }

        if let Some(session_uid) = update.session_uid {
            if self.core.snapshot.session_uid.is_none()
                && self.resume_session_uid == Some(session_uid)
            {
                self.core.online_announced = true;
            }
            self.resume_session_uid = None;
        }

        let core_update = self.core.ingest_with_context(update);
        let timeline_reset = core_update.timeline_reset;
        let mut calls = core_update.calls;
        self.race
            .sync_session(self.core.snapshot.session_uid, &self.core.snapshot);
        if (update.session.is_some() || update.damage.is_some() || update.status.is_some())
            && let Some(call) = self.race.reassess_live_conditions(&self.core.snapshot)
        {
            self.core.radio_revisions.strategy =
                self.core.radio_revisions.strategy.saturating_add(1);
            calls.push(call);
        }
        if timeline_reset.is_some()
            && let Some(current_lap) = self
                .core
                .snapshot
                .lap
                .as_ref()
                .map(|lap| lap.current_lap_num)
        {
            self.race.rewind_to_lap(current_lap);
        }
        if let Some(classification) = update.final_classification.as_ref() {
            self.lifecycle = SessionLifecycle::from_final_classification(classification);
        }
        if self
            .practice
            .sync_session(self.core.snapshot.session_uid, &self.core.snapshot)
        {
            self.last_announced_program = None;
            self.write_practice_state();
        }
        if let Some(lap) = completed_lap {
            self.practice.observe(lap, &self.core.snapshot);
            self.write_practice_state();
            calls.push(EngineerCall::normal(
                "lap_complete",
                format!(
                    "{}번 랩 완료. 랩 타임 {}.",
                    lap.lap_num,
                    format_lap_time(lap.lap_time_ms)
                ),
            ));
            self.core.radio_revisions.strategy =
                self.core.radio_revisions.strategy.saturating_add(1);
            calls.extend(self.race.observe(lap, &self.core.snapshot));
            if let Some(program) = self.practice.plan(&self.core.snapshot) {
                let instruction = program
                    .instructions
                    .first()
                    .map(String::as_str)
                    .unwrap_or("조건을 유지해 주행");
                let key =
                    ProgramAnnouncementKey::from_program(self.core.snapshot.session_uid, &program);
                if self.last_announced_program.as_ref() != Some(&key) {
                    self.last_announced_program = Some(key.clone());
                    self.practice.last_announced_program = Some(key);
                    self.write_practice_state();
                    calls.push(EngineerCall::normal(
                        "practice_program",
                        format!(
                            "다음 프로그램: {}. {}.",
                            program.objective,
                            instruction.trim_end_matches('.')
                        ),
                    ));
                }
            }
        }

        if let Some(voice) = &self.voice {
            voice.synchronize(self.core.radio_scope(&self.source));
        }

        let force_write = !calls.is_empty() || update.final_classification.is_some();
        self.write_live_state(force_write);
        self.write_history(force_write);
        if !calls.is_empty() {
            self.write_trigger(&calls, completed_lap, timeline_reset.as_ref(), true);
        }

        for call in calls {
            let should_speak = self.should_speak(&call);
            self.emit_call(&call, timeline_reset.as_ref());

            if should_speak && let Some(voice) = &self.voice {
                voice.say(VoiceJob {
                    queued_at_unix_ms: unix_ms(),
                    source: self.source.clone(),
                    session_uid: self.core.snapshot.session_uid,
                    timeline_revision: self.core.timeline_revision,
                    state_revision: self.core.radio_revision(call.kind),
                    session_type: self
                        .core
                        .snapshot
                        .session
                        .as_ref()
                        .map(|session| session.session_type),
                    lap: self
                        .core
                        .snapshot
                        .lap
                        .as_ref()
                        .map(|lap| lap.current_lap_num),
                    position: self.core.snapshot.lap.as_ref().map(|lap| lap.car_position),
                    priority: call.priority,
                    kind: call.kind,
                    message: call.message.clone(),
                });
            }
        }
    }

    pub fn finish_session(&mut self, reason: &'static str) {
        self.finish_session_internal(reason);
    }

    fn finish_session_internal(&mut self, reason: &'static str) {
        let has_activity = self.core.snapshot.session_uid.is_some()
            || self.core.snapshot.input.is_some()
            || self.core.snapshot.lap.is_some()
            || self.core.snapshot.session.is_some();
        if !has_activity || self.lifecycle.is_terminal() {
            return;
        }

        self.lifecycle = SessionLifecycle {
            status: SessionLifecycleStatus::Interrupted,
            ended_at_unix_ms: Some(unix_ms()),
            end_reason: Some(reason),
        };
        self.core.driving = false;
        self.core.radio_revisions.invalidate_all();
        if let Some(voice) = &self.voice {
            voice.synchronize(self.core.radio_scope(&self.source));
        }

        let message = match reason {
            "session_replaced" => "새 세션이 시작돼 이전 세션 기록을 종료했다.",
            "source_replaced" => "텔레메트리 소스가 바뀌어 이전 세션 기록을 종료했다.",
            _ => "브리지 종료로 세션 기록을 중도 종료했다.",
        };
        let call = EngineerCall::important("session_interrupted", message);
        self.write_live_state(true);
        self.write_history(true);
        self.write_trigger(std::slice::from_ref(&call), None, None, false);
        self.emit_call(&call, None);
    }

    fn emit_call(&mut self, call: &EngineerCall, timeline_reset: Option<&TimelineReset>) {
        if self.ai_managed {
            println!(
                "[engineer-observation][{}][{}][{}]",
                self.source,
                call.priority.label(),
                call.kind
            );
        } else {
            println!(
                "[engineer][{}][{}][{}] {}",
                self.source,
                call.priority.label(),
                call.kind,
                call.message
            );
        }

        let Some(log) = &mut self.log else {
            return;
        };
        let logged = LoggedCall {
            schema_version: 2,
            timestamp_unix_ms: unix_ms(),
            source: &self.source,
            session_uid: self.core.snapshot.session_uid,
            session_time: self.core.last_session_time,
            timeline_revision: self.core.timeline_revision,
            timeline_reset: (call.kind == "timeline_reset")
                .then_some(timeline_reset)
                .flatten(),
            priority: call.priority,
            kind: call.kind,
            message: &call.message,
        };
        if let Err(error) = serde_json::to_writer(&mut *log, &logged)
            .and_then(|()| log.write_all(b"\n").map_err(serde_json::Error::io))
            .and_then(|()| log.flush().map_err(serde_json::Error::io))
        {
            eprintln!("[engineer-warning] failed to write engineer log: {error}; disabling log");
            self.log = None;
        }
    }

    fn should_speak(&self, call: &EngineerCall) -> bool {
        if call_is_voice_suppressed(call.kind, self.ai_managed) {
            return false;
        }
        let practice = self
            .core
            .snapshot
            .session
            .as_ref()
            .is_some_and(|session| matches!(session.session_type, 1..=4));
        if !practice {
            return true;
        }

        call.priority == CallPriority::Critical
            || matches!(
                call.kind,
                "practice_program"
                    | "lap_invalid"
                    | "yellow_flag"
                    | "red_flag"
                    | "green_flag"
                    | "session_finished"
            )
    }

    fn write_live_state(&mut self, force: bool) {
        if !force && self.last_state_write.elapsed() < Duration::from_millis(200) {
            return;
        }
        let Some(path) = self.state_path.as_ref() else {
            return;
        };
        let json = serde_json::to_vec_pretty(&self.live_state());
        match json
            .map_err(|error| error.to_string())
            .and_then(|json| write_atomic(path, &json))
        {
            Ok(()) => self.last_state_write = Instant::now(),
            Err(error) => {
                eprintln!("[engineer-warning] {error}; disabling live state output");
                self.state_path = None;
            }
        }
    }

    fn write_practice_state(&mut self) {
        let Some(path) = self.practice_state_path.as_ref() else {
            return;
        };
        let json = match serde_json::to_vec_pretty(&self.practice) {
            Ok(json) => json,
            Err(error) => {
                eprintln!("[engineer-warning] failed to serialize practice advisor: {error}");
                return;
            }
        };
        if let Err(error) = write_atomic(path, &json) {
            eprintln!("[engineer-warning] {error}; disabling practice advisor persistence");
            self.practice_state_path = None;
        }
    }

    fn write_history(&mut self, force: bool) {
        if !force && self.last_history_write.elapsed() < Duration::from_millis(200) {
            return;
        }
        if !self.core.driving
            && self.core.snapshot.final_classification.is_none()
            && !self.lifecycle.is_terminal()
        {
            return;
        }
        let json = match serde_json::to_vec(&self.live_state()) {
            Ok(json) => json,
            Err(error) => {
                eprintln!("[engineer-warning] failed to serialize state history: {error}");
                self.history = None;
                return;
            }
        };
        let Some(history) = &mut self.history else {
            return;
        };
        match history
            .write_all(&json)
            .and_then(|()| history.write_all(b"\n"))
            .and_then(|()| history.flush())
        {
            Ok(()) => self.last_history_write = Instant::now(),
            Err(error) => {
                eprintln!(
                    "[engineer-warning] failed to write engineer state history: {error}; disabling history"
                );
                self.history = None;
            }
        }
    }

    fn write_trigger(
        &mut self,
        calls: &[EngineerCall],
        completed_lap: Option<&CompletedLapAnalysis>,
        timeline_reset: Option<&TimelineReset>,
        notify_hook: bool,
    ) {
        let Some(path) = self.trigger_path.as_ref() else {
            return;
        };
        self.trigger_sequence = self.trigger_sequence.saturating_add(1);
        let trigger = EngineerTrigger {
            schema_version: 4,
            sequence: self.trigger_sequence,
            timestamp_unix_ms: unix_ms(),
            timeline_revision: self.core.timeline_revision,
            timeline_reset,
            decision_mode: if self.ai_managed { "ai" } else { "rules" },
            reasons: calls.iter().map(|call| call.kind).collect(),
            calls,
            completed_lap: completed_lap.map(CompletedLapTrigger::from),
            state_path: self.state_path.as_deref(),
            history_path: self.history_path.as_deref(),
            input_log_path: self.input_log_path.as_deref(),
            corner_log_path: self.corner_log_path.as_deref(),
            analysis_report_path: self.analysis_report_path.as_deref(),
            radio_path: self.radio_path.as_deref(),
            practice_state_path: self.practice_state_path.as_deref(),
            state: self.live_state(),
        };
        let json = match serde_json::to_vec_pretty(&trigger) {
            Ok(json) => json,
            Err(error) => {
                eprintln!("[engineer-warning] failed to serialize engineer trigger: {error}");
                return;
            }
        };
        match write_atomic(path, &json) {
            Ok(()) => {
                let should_notify = notify_hook
                    && (!self.ai_managed || calls.iter().any(|call| ai_wake_event(call.kind)));
                if should_notify && let Some(hook) = &self.hook {
                    hook.trigger_snapshot(self.trigger_sequence, json.clone());
                }
            }
            Err(error) => {
                eprintln!("[engineer-warning] {error}; disabling event trigger output");
                self.trigger_path = None;
                self.hook = None;
            }
        }
    }

    fn live_state(&self) -> LiveEngineerState<'_> {
        LiveEngineerState {
            schema_version: 8,
            updated_at_unix_ms: unix_ms(),
            source: &self.source,
            packet_format: self.core.snapshot.packet_format,
            session_uid: self.core.snapshot.session_uid,
            timeline_revision: self.core.timeline_revision,
            radio_revisions: self.core.radio_revisions,
            last_timeline_reset: self.core.last_timeline_reset.as_ref(),
            lifecycle: &self.lifecycle,
            decision_mode: if self.ai_managed { "ai" } else { "rules" },
            session_type_name: self
                .core
                .snapshot
                .session
                .as_ref()
                .map(|session| session_type_name(session.session_type)),
            radio_path: self.radio_path.as_deref(),
            practice_state_path: self.practice_state_path.as_deref(),
            driving: self.core.driving,
            input: self.core.snapshot.input.as_ref(),
            lap: self.core.snapshot.lap.as_ref(),
            session: self.core.snapshot.session.as_ref(),
            damage: self.core.snapshot.damage.as_ref(),
            status: self.core.snapshot.status.as_ref(),
            setup: self.core.snapshot.setup.as_ref(),
            tyre_sets: self.core.snapshot.tyre_sets.as_ref(),
            ers: self.core.snapshot.status.as_ref().map(ErsSummary::from),
            race_strategy: self.race.summary(&self.core.snapshot),
            practice_program: self.practice.plan(&self.core.snapshot),
            final_classification: self.core.snapshot.final_classification.as_ref(),
        }
    }
}

fn call_is_voice_suppressed(kind: &str, ai_managed: bool) -> bool {
    ai_managed
        || matches!(
            kind,
            "lap_complete"
                | "engineer_online"
                | "timeline_reset"
                | "position"
                | "race_strategy_snapshot"
                | "front_gap"
                | "behind_gap"
        )
}

fn ai_wake_event(kind: &str) -> bool {
    !matches!(
        kind,
        "engineer_online"
            | "timeline_reset"
            | "position"
            | "front_gap"
            | "behind_gap"
            | "race_strategy_snapshot"
    )
}

#[derive(Serialize)]
struct LiveEngineerState<'a> {
    schema_version: u8,
    updated_at_unix_ms: u128,
    source: &'a str,
    packet_format: Option<u16>,
    session_uid: Option<u64>,
    timeline_revision: u64,
    radio_revisions: RadioStateRevisions,
    last_timeline_reset: Option<&'a TimelineReset>,
    lifecycle: &'a SessionLifecycle,
    decision_mode: &'static str,
    session_type_name: Option<&'static str>,
    radio_path: Option<&'a Path>,
    practice_state_path: Option<&'a Path>,
    driving: bool,
    input: Option<&'a InputSample>,
    lap: Option<&'a LapSample>,
    session: Option<&'a SessionSample>,
    damage: Option<&'a DamageSample>,
    status: Option<&'a StatusSample>,
    setup: Option<&'a CarSetupSample>,
    tyre_sets: Option<&'a TyreSetsSample>,
    ers: Option<ErsSummary>,
    race_strategy: Option<RaceStrategySummary>,
    practice_program: Option<PracticeProgram>,
    final_classification: Option<&'a FinalClassificationSample>,
}

impl Drop for RaceEngineer {
    fn drop(&mut self) {
        self.finish_session_internal("bridge_shutdown");
    }
}
