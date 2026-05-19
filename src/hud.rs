use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::telemetry::InputSample;

#[derive(Clone)]
pub struct HudHandle {
    state: Arc<Mutex<Option<InputSample>>>,
}

impl HudHandle {
    pub fn update(&self, sample: InputSample) {
        if let Ok(mut state) = self.state.lock() {
            *state = Some(sample);
        }
    }
}

pub fn start_hud_server(host: &str, port: u16) -> Result<HudHandle, String> {
    let listener = TcpListener::bind(format!("{host}:{port}"))
        .map_err(|error| format!("HUD bind failed: {error}"))?;
    let state = Arc::new(Mutex::new(None));
    let thread_state = Arc::clone(&state);

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = Arc::clone(&thread_state);
            thread::spawn(move || {
                let _ = handle_connection(stream, state);
            });
        }
    });

    Ok(HudHandle { state })
}

fn handle_connection(
    mut stream: TcpStream,
    state: Arc<Mutex<Option<InputSample>>>,
) -> Result<(), String> {
    let mut buffer = [0_u8; 1024];
    let size = stream
        .read(&mut buffer)
        .map_err(|error| format!("HUD request read failed: {error}"))?;
    let request = String::from_utf8_lossy(&buffer[..size]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    if path == "/state" {
        let body = state
            .lock()
            .ok()
            .and_then(|sample| sample.as_ref().map(InputSample::to_json))
            .unwrap_or_else(|| "{}".to_owned());
        return write_response(
            &mut stream,
            "200 OK",
            "application/json",
            "Cache-Control: no-store\r\n",
            &body,
        );
    }

    write_response(
        &mut stream,
        "200 OK",
        "text/html; charset=utf-8",
        "",
        HUD_HTML,
    )
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    extra_headers: &str,
    body: &str,
) -> Result<(), String> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra_headers}\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| format!("HUD response write failed: {error}"))
}

const HUD_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>F1 MOZA Bridge HUD</title>
  <style>
    :root { color-scheme: dark; font-family: Inter, system-ui, sans-serif; background: #05070a; color: #e8edf2; }
    body { margin: 0; min-height: 100vh; display: grid; place-items: center; }
    main { width: min(920px, 94vw); display: grid; gap: 22px; }
    .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 18px; }
    .label { font-size: 18px; color: #7e8792; letter-spacing: .04em; }
    .value { font-size: clamp(46px, 8vw, 92px); font-weight: 800; line-height: .95; }
    .bar { height: 42px; background: #161b22; border: 1px solid #2d333b; overflow: hidden; }
    .fill { height: 100%; width: 0%; transition: width 16ms linear; }
    #throttle { background: #21d17c; }
    #brake { background: #ff3b30; }
    .stats { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; }
    .stat { border-top: 1px solid #2d333b; padding-top: 10px; }
    .stat b { display: block; font-size: 32px; }
  </style>
</head>
<body>
<main>
  <section>
    <div class="label">THROTTLE</div>
    <div class="value" id="throttleValue">0%</div>
    <div class="bar"><div class="fill" id="throttle"></div></div>
  </section>
  <section>
    <div class="label">BRAKE</div>
    <div class="value" id="brakeValue">0%</div>
    <div class="bar"><div class="fill" id="brake"></div></div>
  </section>
  <section class="stats">
    <div class="stat"><span class="label">SPEED</span><b id="speed">0</b></div>
    <div class="stat"><span class="label">GEAR</span><b id="gear">N</b></div>
    <div class="stat"><span class="label">RPM</span><b id="rpm">0</b></div>
    <div class="stat"><span class="label">FRAME</span><b id="frame">0</b></div>
  </section>
</main>
<script>
const pct = value => Math.max(0, Math.min(100, Math.round((value || 0) * 100)));
async function tick() {
  try {
    const sample = await fetch('/state', { cache: 'no-store' }).then(r => r.json());
    const throttle = pct(sample.throttle);
    const brake = pct(sample.brake);
    document.getElementById('throttleValue').textContent = throttle + '%';
    document.getElementById('brakeValue').textContent = brake + '%';
    document.getElementById('throttle').style.width = throttle + '%';
    document.getElementById('brake').style.width = brake + '%';
    document.getElementById('speed').textContent = sample.speedKmh ?? 0;
    document.getElementById('gear').textContent = sample.gear ?? 'N';
    document.getElementById('rpm').textContent = sample.rpm ?? 0;
    document.getElementById('frame').textContent = sample.frameIdentifier ?? 0;
  } catch (_) {}
  setTimeout(() => requestAnimationFrame(tick), 16);
}
requestAnimationFrame(tick);
</script>
</body>
</html>
"#;
