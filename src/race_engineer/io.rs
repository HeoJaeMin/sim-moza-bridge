fn open_log(path: &str) -> Result<BufWriter<std::fs::File>, String> {
    open_jsonl(path, "race engineer event log")
}

fn open_jsonl(path: &str, label: &str) -> Result<BufWriter<std::fs::File>, String> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(Path::new(path))
        .map_err(|error| format!("failed to open {label} {path}: {error}"))?;
    if metadata(path).map(|value| value.len()).unwrap_or_default() == 0 {
        println!("{label}: {path}");
    }
    Ok(BufWriter::new(file))
}

fn load_live_state_session_uid(path: &Path) -> Option<u64> {
    if !path.exists() {
        return None;
    }
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|state| state.get("session_uid").and_then(serde_json::Value::as_u64))
}

fn load_practice_advisor(path: &Path) -> Option<PracticeAdvisor> {
    if !path.exists() {
        return None;
    }
    match std::fs::read(path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()))
    {
        Ok(state) => Some(state),
        Err(error) => {
            eprintln!(
                "[engineer-warning] failed to load practice advisor {}: {error}",
                path.display()
            );
            None
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("engineer.json");
    let temporary_path = path.with_file_name(format!(".{file_name}.tmp"));
    let mut temporary = std::fs::File::create(&temporary_path).map_err(|error| {
        format!(
            "failed to create temporary output {}: {error}",
            temporary_path.display()
        )
    })?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.flush())
        .map_err(|error| {
            format!(
                "failed to write temporary output {}: {error}",
                temporary_path.display()
            )
        })?;
    drop(temporary);

    #[cfg(windows)]
    if path.exists() {
        std::fs::remove_file(path)
            .map_err(|error| format!("failed to replace {}: {error}", path.display()))?;
    }
    std::fs::rename(&temporary_path, path).map_err(|error| {
        format!(
            "failed to publish {} from {}: {error}",
            path.display(),
            temporary_path.display()
        )
    })
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[derive(Clone)]
struct HookInvocation {
    sequence: u64,
    snapshot: Option<Vec<u8>>,
}

#[derive(Default)]
struct PendingHookInvocation {
    latest: Option<HookInvocation>,
    closed: bool,
}

struct HookWorker {
    pending: Arc<(Mutex<PendingHookInvocation>, Condvar)>,
    next_sequence: AtomicU64,
}

#[derive(Clone)]
struct HookEnvironment {
    name: &'static str,
    value: String,
}

impl HookWorker {
    fn start(
        hook_path: PathBuf,
        payload_path: PathBuf,
        payload_env: &'static str,
        environment: Option<HookEnvironment>,
        timeout: Duration,
    ) -> Self {
        let pending = Arc::new((Mutex::new(PendingHookInvocation::default()), Condvar::new()));
        let worker_pending = Arc::clone(&pending);
        if let Err(error) = thread::Builder::new()
            .name("race-engineer-hook".to_owned())
            .spawn(move || {
                loop {
                    let invocation = {
                        let (state, available) = &*worker_pending;
                        let mut state = state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        while state.latest.is_none() && !state.closed {
                            state = available
                                .wait(state)
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                        }
                        if state.closed {
                            return;
                        }
                        state.latest.take().expect("hook invocation is available")
                    };

                    let immutable_path = invocation.snapshot.as_ref().map(|snapshot| {
                        let path = hook_snapshot_path(&payload_path, invocation.sequence);
                        (path, snapshot)
                    });
                    let invocation_path = if let Some((path, snapshot)) = &immutable_path {
                        if let Err(error) = write_atomic(path, snapshot) {
                            eprintln!("[engineer-warning] failed to stage hook payload: {error}");
                            continue;
                        }
                        path.as_path()
                    } else {
                        payload_path.as_path()
                    };
                    if let Err(error) = run_hook(
                        &hook_path,
                        invocation_path,
                        payload_env,
                        environment.as_ref(),
                        timeout,
                    ) {
                        eprintln!("[engineer-warning] hook failed: {error}");
                    }
                    if let Some((path, _)) = immutable_path {
                        let _ = std::fs::remove_file(path);
                    }
                }
            })
        {
            eprintln!("[engineer-warning] failed to create event hook worker: {error}");
        }
        Self {
            pending,
            next_sequence: AtomicU64::new(0),
        }
    }

    fn trigger(&self) {
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        self.enqueue(HookInvocation {
            sequence,
            snapshot: None,
        });
    }

    fn trigger_snapshot(&self, sequence: u64, snapshot: Vec<u8>) {
        self.enqueue(HookInvocation {
            sequence,
            snapshot: Some(snapshot),
        });
    }

    fn enqueue(&self, invocation: HookInvocation) {
        let (state, available) = &*self.pending;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return;
        }
        state.latest = Some(invocation);
        available.notify_one();
    }
}

impl Drop for HookWorker {
    fn drop(&mut self) {
        let (state, available) = &*self.pending;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.closed = true;
        state.latest = None;
        available.notify_all();
    }
}

fn hook_snapshot_path(payload_path: &Path, sequence: u64) -> PathBuf {
    let file_name = payload_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("engineer-trigger.json");
    payload_path.with_file_name(format!(".{file_name}.hook-{sequence}.json"))
}

fn run_hook(
    hook_path: &Path,
    payload_path: &Path,
    payload_env: &str,
    environment: Option<&HookEnvironment>,
    timeout: Duration,
) -> Result<(), String> {
    let mut command = prepare_hook_command(hook_path, payload_path, payload_env, environment);
    let mut child = command.spawn().map_err(|error| {
        format!(
            "could not start {} for {}: {error}",
            hook_path.display(),
            payload_path.display()
        )
    })?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "{} exited with status {status}",
                    hook_path.display()
                ));
            }
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} exceeded the {} second deadline",
                    hook_path.display(),
                    timeout.as_secs()
                ));
            }
            Err(error) => {
                return Err(format!(
                    "could not poll {} for {}: {error}",
                    hook_path.display(),
                    payload_path.display()
                ));
            }
        }
    }
}

fn prepare_hook_command(
    hook_path: &Path,
    payload_path: &Path,
    payload_env: &str,
    environment: Option<&HookEnvironment>,
) -> Command {
    let is_powershell = hook_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ps1"));
    let mut command = if cfg!(windows) && is_powershell {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ]);
        command.arg(hook_path);
        command
    } else {
        Command::new(hook_path)
    };
    command
        .arg(payload_path)
        .env(payload_env, payload_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(environment) = environment {
        command.env(environment.name, &environment.value);
    }
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    command
}
