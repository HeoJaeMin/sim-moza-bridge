use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::telemetry::InputSample;

const HUD_READ_TIMEOUT: Duration = Duration::from_secs(2);

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
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let state = Arc::clone(&thread_state);
                    thread::spawn(move || {
                        if let Err(error) = handle_connection(stream, state) {
                            eprintln!("[hud-error] {error}");
                        }
                    });
                }
                Err(error) => eprintln!("[hud-error] accept failed: {error}"),
            }
        }
    });

    Ok(HudHandle { state })
}

fn handle_connection(
    mut stream: TcpStream,
    state: Arc<Mutex<Option<InputSample>>>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(HUD_READ_TIMEOUT))
        .map_err(|error| format!("HUD timeout setup failed: {error}"))?;
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
  <title>Sim MOZA Bridge HUD</title>
  <style>
    :root { color-scheme: dark; font-family: Inter, ui-sans-serif, system-ui, sans-serif; background: #07090d; color: #edf2f7; }
    * { box-sizing: border-box; }
    body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: #07090d; }
    main { width: min(1100px, 96vw); display: grid; gap: 18px; padding: 18px 0; }
    .top { display: grid; grid-template-columns: minmax(160px, .7fr) minmax(220px, 1fr) minmax(160px, .7fr); gap: 16px; align-items: stretch; }
    .gear { display: grid; place-items: center; min-height: 150px; border: 1px solid #2e3540; background: #10151c; }
    .gear b { font-size: clamp(84px, 14vw, 156px); line-height: .8; }
    .rpm { display: grid; gap: 14px; align-content: center; min-height: 150px; }
    .leds { display: grid; grid-template-columns: repeat(15, 1fr); gap: 6px; }
    .led { height: 18px; background: #1d2430; border: 1px solid #313a47; }
    .led.on:nth-child(-n+5) { background: #20d47b; }
    .led.on:nth-child(n+6):nth-child(-n+10) { background: #f0d440; }
    .led.on:nth-child(n+11) { background: #ff3b30; }
    .led.flash { animation: flash 90ms steps(1) infinite; }
    @keyframes flash { 50% { opacity: .18; } }
    .revValue { display: flex; justify-content: space-between; font-size: 22px; font-weight: 800; color: #dfe7ef; }
    .stats { display: grid; grid-template-columns: repeat(2, 1fr); gap: 10px; }
    .stat { border-top: 1px solid #2e3540; padding-top: 9px; min-width: 0; }
    .label { font-size: 13px; color: #8b96a4; letter-spacing: 0; }
    .stat b { display: block; margin-top: 3px; font-size: 28px; line-height: 1; overflow-wrap: anywhere; }
    .inputs { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
    .panel { display: grid; gap: 8px; min-width: 0; }
    .row { display: flex; justify-content: space-between; align-items: baseline; gap: 10px; }
    .row b { font-size: 38px; line-height: 1; }
    .bar { height: 38px; background: #151b24; border: 1px solid #2e3540; overflow: hidden; }
    .fill { height: 100%; width: 0%; transition: width 16ms linear; }
    #throttle { background: #21d17c; }
    #brake { background: #ff3b30; }
    .steerWrap { position: relative; height: 38px; background: #151b24; border: 1px solid #2e3540; overflow: hidden; }
    .steerCenter { position: absolute; top: 0; bottom: 0; left: 50%; width: 2px; background: #6a7482; }
    .steerFill { position: absolute; top: 0; bottom: 0; width: 0%; background: #58a6ff; }
    .trace { width: 100%; height: 220px; border: 1px solid #2e3540; background: #0d1218; display: block; }
    @media (max-width: 760px) {
      .top, .inputs { grid-template-columns: 1fr; }
      .stats { grid-template-columns: repeat(4, 1fr); }
      .gear { min-height: 110px; }
      .trace { height: 180px; }
    }
  </style>
</head>
<body>
<main>
  <section class="top">
    <div class="gear"><b id="gear">N</b></div>
    <div class="rpm">
      <div class="leds" id="leds"></div>
      <div class="revValue"><span id="rpm">0 RPM</span><span id="rev">0%</span></div>
    </div>
    <div class="stats">
      <div class="stat"><span class="label">SPEED</span><b id="speed">0</b></div>
      <div class="stat"><span class="label">DRS</span><b id="drs">OFF</b></div>
      <div class="stat"><span class="label">CLUTCH</span><b id="clutch">0</b></div>
      <div class="stat"><span class="label">FRAME</span><b id="frame">0</b></div>
    </div>
  </section>
  <section class="inputs">
    <div class="panel">
      <div class="row"><span class="label">THROTTLE</span><b id="throttleValue">0%</b></div>
      <div class="bar"><div class="fill" id="throttle"></div></div>
    </div>
    <div class="panel">
      <div class="row"><span class="label">BRAKE</span><b id="brakeValue">0%</b></div>
      <div class="bar"><div class="fill" id="brake"></div></div>
    </div>
    <div class="panel">
      <div class="row"><span class="label">STEER</span><b id="steerValue">0%</b></div>
      <div class="steerWrap">
        <div class="steerCenter"></div>
        <div class="steerFill" id="steerLeft"></div>
        <div class="steerFill" id="steerRight"></div>
      </div>
    </div>
    <div class="panel">
      <div class="row"><span class="label">INPUT TRACE</span><b id="sampleCount">0</b></div>
      <canvas class="trace" id="trace" width="1000" height="220"></canvas>
    </div>
  </section>
</main>
<script>
const pct = value => Math.max(0, Math.min(100, Math.round((value || 0) * 100)));
const signedPct = value => Math.max(-100, Math.min(100, Math.round((value || 0) * 100)));
const leds = document.getElementById('leds');
for (let index = 0; index < 15; index += 1) {
  const led = document.createElement('span');
  led.className = 'led';
  leds.appendChild(led);
}
const trace = [];
const canvas = document.getElementById('trace');
const ctx = canvas.getContext('2d');
function drawTrace() {
  const width = canvas.width;
  const height = canvas.height;
  ctx.clearRect(0, 0, width, height);
  ctx.strokeStyle = '#27313d';
  ctx.lineWidth = 1;
  for (let i = 1; i < 4; i += 1) {
    const y = i * height / 4;
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(width, y);
    ctx.stroke();
  }
  drawLine('throttle', '#21d17c', value => height - value * height);
  drawLine('brake', '#ff3b30', value => height - value * height);
  drawLine('steer', '#58a6ff', value => height / 2 - value * height / 2);
}
function drawLine(key, color, yFor) {
  if (trace.length < 2) return;
  ctx.strokeStyle = color;
  ctx.lineWidth = 3;
  ctx.beginPath();
  trace.forEach((sample, index) => {
    const x = index * canvas.width / Math.max(1, trace.length - 1);
    const y = yFor(sample[key] || 0);
    if (index === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
  });
  ctx.stroke();
}
async function tick() {
  try {
    const sample = await fetch('/state', { cache: 'no-store' }).then(r => r.json());
    const throttle = pct(sample.throttle);
    const brake = pct(sample.brake);
    const steer = signedPct(sample.steer);
    const rev = Math.max(0, Math.min(100, sample.revLightsPercent || 0));
    document.getElementById('throttleValue').textContent = throttle + '%';
    document.getElementById('brakeValue').textContent = brake + '%';
    document.getElementById('steerValue').textContent = steer + '%';
    document.getElementById('throttle').style.width = throttle + '%';
    document.getElementById('brake').style.width = brake + '%';
    document.getElementById('steerLeft').style.left = (50 - Math.max(0, -steer) / 2) + '%';
    document.getElementById('steerLeft').style.width = Math.max(0, -steer) / 2 + '%';
    document.getElementById('steerRight').style.left = '50%';
    document.getElementById('steerRight').style.width = Math.max(0, steer) / 2 + '%';
    document.getElementById('speed').textContent = sample.speedKmh ?? 0;
    document.getElementById('gear').textContent = sample.gear ?? 'N';
    document.getElementById('rpm').textContent = (sample.rpm ?? 0) + ' RPM';
    document.getElementById('rev').textContent = Math.round(rev) + '%';
    document.getElementById('drs').textContent = sample.drs ? 'ON' : 'OFF';
    document.getElementById('clutch').textContent = sample.clutch ?? 0;
    document.getElementById('frame').textContent = sample.frameIdentifier ?? 0;
    Array.from(leds.children).forEach((led, index) => {
      led.classList.toggle('on', index < Math.ceil(rev / 100 * 15));
      led.classList.toggle('flash', rev >= 95);
    });
    trace.push({ throttle: (sample.throttle || 0), brake: (sample.brake || 0), steer: (sample.steer || 0) });
    if (trace.length > 180) trace.shift();
    document.getElementById('sampleCount').textContent = trace.length;
    drawTrace();
  } catch (_) {}
  setTimeout(() => requestAnimationFrame(tick), 16);
}
requestAnimationFrame(tick);
</script>
</body>
</html>
"#;
