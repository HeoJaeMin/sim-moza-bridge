use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::telemetry::{
    DamageSample, InputSample, LapSample, SessionSample, StatusSample, TelemetryUpdate,
    WheelValuesF32, WheelValuesU8, WheelValuesU16,
};

const HUD_READ_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct HudHandle {
    state: Arc<Mutex<HudState>>,
}

impl HudHandle {
    pub fn update(&self, update: &TelemetryUpdate) {
        if let Ok(mut state) = self.state.lock() {
            state.apply(update);
        }
    }
}

#[derive(Clone, Debug, Default)]
struct HudState {
    input: Option<InputSample>,
    lap: Option<LapSample>,
    session: Option<SessionSample>,
    damage: Option<DamageSample>,
    status: Option<StatusSample>,
}

impl HudState {
    fn apply(&mut self, update: &TelemetryUpdate) {
        if let Some(input) = &update.input {
            self.input = Some(input.clone());
        }
        if let Some(lap) = &update.lap {
            self.lap = Some(lap.clone());
        }
        if let Some(session) = &update.session {
            self.session = Some(session.clone());
        }
        if let Some(damage) = &update.damage {
            self.damage = Some(damage.clone());
        }
        if let Some(status) = &update.status {
            self.status = Some(status.clone());
        }
    }

    fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"input\":{},",
                "\"lap\":{},",
                "\"session\":{},",
                "\"damage\":{},",
                "\"status\":{}",
                "}}"
            ),
            self.input
                .as_ref()
                .map(input_json)
                .unwrap_or_else(|| "null".to_owned()),
            self.lap
                .as_ref()
                .map(lap_json)
                .unwrap_or_else(|| "null".to_owned()),
            self.session
                .as_ref()
                .map(session_json)
                .unwrap_or_else(|| "null".to_owned()),
            self.damage
                .as_ref()
                .map(damage_json)
                .unwrap_or_else(|| "null".to_owned()),
            self.status
                .as_ref()
                .map(status_json)
                .unwrap_or_else(|| "null".to_owned())
        )
    }
}

pub fn start_hud_server(host: &str, port: u16) -> Result<HudHandle, String> {
    let listener = TcpListener::bind(format!("{host}:{port}"))
        .map_err(|error| format!("HUD bind failed: {error}"))?;
    let state = Arc::new(Mutex::new(HudState::default()));
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

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<HudState>>) -> Result<(), String> {
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
            .map(|state| state.to_json())
            .unwrap_or_else(|_| HudState::default().to_json());
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

fn input_json(sample: &InputSample) -> String {
    format!(
        concat!(
            "{{",
            "\"sessionTime\":{:.3},",
            "\"frameIdentifier\":{},",
            "\"playerCarIndex\":{},",
            "\"throttle\":{:.5},",
            "\"brake\":{:.5},",
            "\"steer\":{:.5},",
            "\"clutch\":{},",
            "\"speedKmh\":{},",
            "\"gear\":{},",
            "\"rpm\":{},",
            "\"drs\":{},",
            "\"revLightsPercent\":{},",
            "\"revLightsBitValue\":{},",
            "\"engineTempC\":{},",
            "\"brakeTempsC\":{},",
            "\"tyreSurfaceTempsC\":{},",
            "\"tyreInnerTempsC\":{},",
            "\"tyrePressuresPsi\":{}",
            "}}"
        ),
        sample.session_time,
        sample.frame_identifier,
        sample.player_car_index,
        sample.throttle,
        sample.brake,
        sample.steer,
        sample.clutch,
        sample.speed_kmh,
        sample.gear,
        sample.rpm,
        sample.drs,
        sample.rev_lights_percent,
        sample.rev_lights_bit_value,
        sample.engine_temp_c,
        wheel_u16_json(&sample.brake_temps_c),
        wheel_u8_json(&sample.tyre_surface_temps_c),
        wheel_u8_json(&sample.tyre_inner_temps_c),
        wheel_f32_json(&sample.tyre_pressures_psi)
    )
}

fn lap_json(sample: &LapSample) -> String {
    format!(
        concat!(
            "{{",
            "\"sessionTime\":{:.3},",
            "\"frameIdentifier\":{},",
            "\"playerCarIndex\":{},",
            "\"lastLapTimeMs\":{},",
            "\"currentLapTimeMs\":{},",
            "\"lapDistanceM\":{:.3},",
            "\"totalDistanceM\":{:.3},",
            "\"carPosition\":{},",
            "\"currentLapNum\":{},",
            "\"pitStatus\":{},",
            "\"sector\":{},",
            "\"currentLapInvalid\":{},",
            "\"driverStatus\":{},",
            "\"resultStatus\":{},",
            "\"deltaToCarInFrontMs\":{},",
            "\"deltaToCarBehindMs\":{},",
            "\"deltaToRaceLeaderMs\":{},",
            "\"sector1TimeMs\":{},",
            "\"sector2TimeMs\":{}",
            "}}"
        ),
        sample.session_time,
        sample.frame_identifier,
        sample.player_car_index,
        sample.last_lap_time_ms,
        sample.current_lap_time_ms,
        sample.lap_distance_m,
        sample.total_distance_m,
        sample.car_position,
        sample.current_lap_num,
        sample.pit_status,
        sample.sector,
        sample.current_lap_invalid,
        sample.driver_status,
        sample.result_status,
        option_u32_json(sample.delta_to_car_in_front_ms),
        option_u32_json(sample.delta_to_car_behind_ms),
        option_u32_json(sample.delta_to_race_leader_ms),
        option_u32_json(sample.sector1_time_ms),
        option_u32_json(sample.sector2_time_ms)
    )
}

fn session_json(sample: &SessionSample) -> String {
    format!(
        concat!(
            "{{",
            "\"sessionTime\":{:.3},",
            "\"frameIdentifier\":{},",
            "\"totalLaps\":{},",
            "\"trackLengthM\":{},",
            "\"sessionType\":{},",
            "\"trackId\":{},",
            "\"trackTempC\":{},",
            "\"airTempC\":{},",
            "\"sessionTimeLeftS\":{},",
            "\"marshalZones\":{}",
            "}}"
        ),
        sample.session_time,
        sample.frame_identifier,
        sample.total_laps,
        sample.track_length_m,
        sample.session_type,
        sample.track_id,
        sample.track_temp_c,
        sample.air_temp_c,
        sample.session_time_left_s,
        marshal_zones_json(&sample.marshal_zones)
    )
}

fn marshal_zones_json(zones: &[crate::telemetry::MarshalZoneSample]) -> String {
    let zones = zones
        .iter()
        .map(|zone| format!("{{\"start\":{:.5},\"flag\":{}}}", zone.start, zone.flag))
        .collect::<Vec<_>>();
    format!("[{}]", zones.join(","))
}

fn damage_json(sample: &DamageSample) -> String {
    format!(
        concat!(
            "{{",
            "\"sessionTime\":{:.3},",
            "\"frameIdentifier\":{},",
            "\"playerCarIndex\":{},",
            "\"tyreWear\":{},",
            "\"tyreDamage\":{},",
            "\"tyreBlisters\":{},",
            "\"frontLeftWingDamage\":{},",
            "\"frontRightWingDamage\":{},",
            "\"rearWingDamage\":{},",
            "\"gearboxDamage\":{},",
            "\"engineDamage\":{}",
            "}}"
        ),
        sample.session_time,
        sample.frame_identifier,
        sample.player_car_index,
        wheel_f32_json(&sample.tyre_wear),
        wheel_u8_json(&sample.tyre_damage),
        wheel_u8_json(&sample.tyre_blisters),
        sample.front_left_wing_damage,
        sample.front_right_wing_damage,
        sample.rear_wing_damage,
        sample.gearbox_damage,
        sample.engine_damage
    )
}

fn status_json(sample: &StatusSample) -> String {
    format!(
        concat!(
            "{{",
            "\"sessionTime\":{:.3},",
            "\"frameIdentifier\":{},",
            "\"playerCarIndex\":{},",
            "\"tractionControl\":{},",
            "\"antiLockBrakes\":{},",
            "\"frontBrakeBias\":{},",
            "\"fuelInTank\":{:.3},",
            "\"fuelCapacity\":{:.3},",
            "\"fuelRemainingLaps\":{:.3},",
            "\"maxRpm\":{},",
            "\"idleRpm\":{},",
            "\"maxGears\":{},",
            "\"drsAllowed\":{},",
            "\"drsActivationDistanceM\":{},",
            "\"pitLimiterActive\":{},",
            "\"actualTyreCompound\":{},",
            "\"visualTyreCompound\":{},",
            "\"tyresAgeLaps\":{},",
            "\"ersStoreEnergy\":{:.3},",
            "\"ersDeployMode\":{},",
            "\"ersDeployedThisLap\":{:.3},",
            "\"ersPercent\":{:.3}",
            "}}"
        ),
        sample.session_time,
        sample.frame_identifier,
        sample.player_car_index,
        sample.traction_control,
        sample.anti_lock_brakes,
        sample.front_brake_bias,
        sample.fuel_in_tank,
        sample.fuel_capacity,
        sample.fuel_remaining_laps,
        sample.max_rpm,
        sample.idle_rpm,
        sample.max_gears,
        sample.drs_allowed,
        sample.drs_activation_distance_m,
        sample.pit_limiter_active,
        sample.actual_tyre_compound,
        sample.visual_tyre_compound,
        sample.tyres_age_laps,
        sample.ers_store_energy,
        sample.ers_deploy_mode,
        sample.ers_deployed_this_lap,
        sample.ers_percent()
    )
}

fn wheel_f32_json(values: &WheelValuesF32) -> String {
    format!(
        "{{\"fl\":{:.3},\"fr\":{:.3},\"rl\":{:.3},\"rr\":{:.3}}}",
        values.fl, values.fr, values.rl, values.rr
    )
}

fn wheel_u8_json(values: &WheelValuesU8) -> String {
    format!(
        "{{\"fl\":{},\"fr\":{},\"rl\":{},\"rr\":{}}}",
        values.fl, values.fr, values.rl, values.rr
    )
}

fn wheel_u16_json(values: &WheelValuesU16) -> String {
    format!(
        "{{\"fl\":{},\"fr\":{},\"rl\":{},\"rr\":{}}}",
        values.fl, values.fr, values.rl, values.rr
    )
}

fn option_u32_json(value: Option<u32>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}

const HUD_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Sim MOZA Bridge Dashboard</title>
  <style>
    :root {
      color-scheme: dark;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: #090907;
      color: #f2efe7;
      --bg: #090907;
      --panel: #15140f;
      --panel-soft: #1d1b14;
      --line: #393528;
      --text: #f2efe7;
      --muted: #a7a092;
      --green: #21d17c;
      --red: #ff4d45;
      --amber: #f0c247;
      --cyan: #51d6d0;
      --violet: #b58cff;
    }
    * { box-sizing: border-box; }
    body { margin: 0; min-height: 100vh; background: var(--bg); color: var(--text); }
    main { width: min(1460px, calc(100vw - 28px)); margin: 0 auto; padding: 14px 0 18px; display: grid; gap: 12px; }
    .barTop, .hero, .grid { display: grid; gap: 12px; }
    .barTop { grid-template-columns: 1.2fr repeat(6, minmax(88px, .5fr)); align-items: stretch; }
    .hero { grid-template-columns: minmax(180px, .7fr) minmax(280px, 1fr) minmax(320px, 1.4fr); align-items: stretch; }
    .grid { grid-template-columns: 1fr 1.05fr 1fr; align-items: stretch; }
    .panel, .metric, .speed, .gear, .rpmPanel {
      border: 1px solid var(--line);
      background: var(--panel);
      border-radius: 6px;
      min-width: 0;
    }
    .metric { padding: 10px 12px; display: grid; align-content: center; gap: 4px; }
    .metric strong, .label, .smallLabel { color: var(--muted); font-size: 12px; font-weight: 800; letter-spacing: 0; }
    .metric b { font-size: 24px; line-height: 1; overflow-wrap: anywhere; }
    .statusLine { padding: 10px 12px; display: flex; align-items: center; gap: 10px; min-width: 0; }
    .brand { font-weight: 900; font-size: 18px; white-space: nowrap; }
    .pill { padding: 5px 8px; border: 1px solid var(--line); background: var(--panel-soft); border-radius: 999px; color: var(--muted); font-size: 12px; font-weight: 900; }
    .pill.live { color: #07120d; background: var(--green); border-color: var(--green); }
    .pill.warn { color: #150d06; background: var(--amber); border-color: var(--amber); }
    .speed { padding: 16px; display: grid; align-content: center; gap: 4px; }
    .speed b { font-size: clamp(76px, 12vw, 168px); line-height: .8; }
    .speed span { color: var(--muted); font-weight: 800; }
    .gear { display: grid; place-items: center; min-height: 210px; }
    .gear b { font-size: clamp(112px, 18vw, 220px); line-height: .78; }
    .rpmPanel { padding: 16px; display: grid; align-content: center; gap: 14px; }
    .leds { display: grid; grid-template-columns: repeat(15, 1fr); gap: 5px; }
    .led { height: 20px; background: #28251b; border: 1px solid #484231; }
    .led.on:nth-child(-n+5) { background: var(--green); }
    .led.on:nth-child(n+6):nth-child(-n+10) { background: var(--amber); }
    .led.on:nth-child(n+11) { background: var(--red); }
    .led.flash { animation: flash 90ms steps(1) infinite; }
    @keyframes flash { 50% { opacity: .18; } }
    .rpmReadout { display: flex; justify-content: space-between; align-items: baseline; gap: 16px; }
    .rpmReadout b { font-size: 40px; line-height: 1; }
    .rpmReadout span { color: var(--muted); font-size: 22px; font-weight: 900; }
    .rpmValue { display: flex; align-items: baseline; justify-content: flex-end; gap: 16px; min-width: 0; }
    .speedInline { display: none; align-content: end; gap: 2px; min-width: 82px; }
    .speedInline span { color: var(--muted); font-size: 10px; font-weight: 900; }
    .speedInline b { font-size: 32px; }
    .gapHeroPanel { align-content: stretch; }
    .gapHeroPanel .kv { align-content: center; }
    .gapHeroPanel .kv b { font-size: 38px; }
    .panel { padding: 12px; display: grid; gap: 12px; }
    .panel h2 { margin: 0; font-size: 13px; color: var(--muted); letter-spacing: 0; text-transform: uppercase; }
    .inputRows { display: grid; gap: 10px; }
    .row { display: grid; gap: 5px; }
    .rowHead { display: flex; justify-content: space-between; align-items: baseline; gap: 10px; }
    .rowHead b { font-size: 22px; line-height: 1; }
    .track { height: 26px; border: 1px solid var(--line); background: #0f100c; overflow: hidden; }
    .fill { height: 100%; width: 0%; transition: width 16ms linear; }
    #throttle { background: var(--green); }
    #brake { background: var(--red); }
    #clutchBar { background: var(--cyan); }
    #fuelBar { background: var(--amber); }
    #ersBar { background: var(--violet); }
    .steerWrap { position: relative; height: 26px; border: 1px solid var(--line); background: #0f100c; overflow: hidden; }
    .steerCenter { position: absolute; top: 0; bottom: 0; left: 50%; width: 2px; background: #69614c; }
    .steerFill { position: absolute; top: 0; bottom: 0; width: 0%; background: var(--cyan); }
    .trace { width: 100%; height: 180px; border: 1px solid var(--line); background: #0f100c; display: block; }
    .twoCols { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
    .kv { display: grid; grid-template-columns: 1fr auto; gap: 10px; border-top: 1px solid var(--line); padding-top: 8px; }
    .kv b { font-size: 17px; line-height: 1.1; text-align: right; }
    .wheels { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
    .wheel { border: 1px solid var(--line); background: var(--panel-soft); border-radius: 6px; padding: 10px; display: grid; gap: 8px; min-width: 0; }
    .wheelTop { display: flex; justify-content: space-between; align-items: baseline; gap: 8px; }
    .wheelTop b { font-size: 22px; line-height: 1; }
    .mini { display: grid; grid-template-columns: repeat(2, 1fr); gap: 6px; }
    .mini div { border-top: 1px solid var(--line); padding-top: 5px; min-width: 0; }
    .mini b { display: block; margin-top: 2px; font-size: 16px; line-height: 1; overflow-wrap: anywhere; }
    .damageBars { display: grid; grid-template-columns: repeat(5, 1fr); gap: 8px; align-items: end; min-height: 148px; }
    .damageBar { display: grid; gap: 6px; text-align: center; color: var(--muted); font-size: 11px; font-weight: 800; min-width: 0; }
    .damageTrack { height: 94px; border: 1px solid var(--line); background: #0f100c; display: flex; align-items: end; overflow: hidden; }
    .damageFill { width: 100%; height: 0%; background: var(--red); transition: height 120ms linear; }
    .notice { color: var(--muted); font-size: 12px; }
    .coreOnly { display: none; }
    .flag-green b, .sector-green b { color: var(--green); }
    .flag-blue b { color: var(--cyan); }
    .flag-yellow b, .sector-yellow b { color: var(--amber); }
    .flag-red b { color: var(--red); }
    .sector-purple b { color: var(--violet); }
    .stale .panel, .stale .metric, .stale .speed, .stale .gear, .stale .rpmPanel { opacity: .72; }
    @media (max-width: 1100px) {
      .barTop { grid-template-columns: repeat(3, 1fr); }
      .statusLine { grid-column: 1 / -1; }
      .hero, .grid { grid-template-columns: 1fr; }
      .gear { min-height: 160px; }
    }
    @media (max-width: 620px) {
      main { width: min(100vw - 18px, 1460px); }
      .barTop, .twoCols, .wheels { grid-template-columns: 1fr 1fr; }
      .statusLine { grid-column: 1 / -1; }
      .brand { font-size: 15px; }
      .metric b { font-size: 20px; }
      .speed b { font-size: 86px; }
      .gear b { font-size: 132px; }
      .rpmReadout b { font-size: 28px; }
      .damageBars { grid-template-columns: repeat(3, 1fr); }
    }
    main.mode-1920x1080 {
      width: min(1888px, calc(100vw - 32px));
      min-height: calc(100vh - 20px);
      grid-template-rows: auto 300px minmax(0, 1fr);
      padding: 10px 0;
    }
    .mode-1920x1080 .hero { grid-template-columns: .72fr .72fr 1.56fr; }
    .mode-1920x1080 .grid { grid-template-columns: .9fr 1.25fr 1fr; }
    .mode-1920x1080 .speed { display: none; }
    .mode-1920x1080 .gapHeroPanel { display: grid; padding: 12px; gap: 12px; min-height: 300px; }
    .mode-1920x1080 .gapHeroPanel .kv b { font-size: 44px; }
    .mode-1920x1080 .gear, .mode-1920x1080 .rpmPanel { min-height: 300px; }
    .mode-1920x1080 .speedInline { display: grid; }
    .mode-1920x1080 .rpmReadout { align-items: baseline; }
    .mode-1920x1080 .rpmValue { margin-left: auto; }
    .mode-1920x1080 .panel { padding: 10px; gap: 10px; }
    .mode-1920x1080 .trace { height: 156px; }
    .mode-1920x1080 .damageBars { min-height: 118px; }
    .mode-1920x1080 .damageTrack { height: 72px; }
    .mode-1920x1080 .wheel { padding: 8px; gap: 7px; }
    .mode-1920x1080 .mini b { font-size: 15px; }

    main.mode-1080x1920 {
      width: min(1056px, calc(100vw - 20px));
      height: calc(100vh - 12px);
      padding: 6px 0;
      gap: 8px;
      grid-template-rows: auto 300px minmax(0, 1fr);
    }
    .mode-1080x1920 .barTop, .mode-1080x1920 .hero, .mode-1080x1920 .grid { gap: 8px; }
    .mode-1080x1920 .barTop { grid-template-columns: 1.2fr 1fr 1fr 1fr; }
    .mode-1080x1920 .statusLine { grid-column: auto; padding: 8px; }
    .mode-1080x1920 .sessionMetric, .mode-1080x1920 .trackMetric, .mode-1080x1920 .airMetric, .mode-1080x1920 .inputPanel { display: none; }
    .mode-1080x1920 .metric { padding: 8px; }
    .mode-1080x1920 .metric b { font-size: 22px; }
    .mode-1080x1920 .hero { grid-template-columns: .65fr .65fr 1.35fr; }
    .mode-1080x1920 .rpmPanel { grid-column: auto; min-height: 300px; padding: 12px; gap: 10px; }
    .mode-1080x1920 .speed, .mode-1080x1920 .gear { min-height: 300px; }
    .mode-1080x1920 .speed b { font-size: 112px; }
    .mode-1080x1920 .gear b { font-size: 160px; }
    .mode-1080x1920 .trace { height: 140px; }
    .mode-1080x1920 .grid { grid-template-columns: 1fr 1fr; min-height: 0; }
    .mode-1080x1920 .panel { padding: 10px; gap: 10px; min-height: 0; }
    .mode-1080x1920 .wheels { grid-template-columns: repeat(2, 1fr); gap: 8px; }
    .mode-1080x1920 .wheel { padding: 8px; gap: 6px; }
    .mode-1080x1920 .mini { gap: 5px; }
    .mode-1080x1920 .mini b { font-size: 14px; }
    .mode-1080x1920 .damageBars { min-height: 108px; }
    .mode-1080x1920 .damageTrack { height: 68px; }

    main.mode-1080x960 {
      width: min(1060px, calc(100vw - 18px));
      height: calc(100vh - 8px);
      padding: 4px 0;
      gap: 8px;
      grid-template-rows: 50px 300px minmax(0, 1fr);
    }
    .mode-1080x960 .barTop, .mode-1080x960 .hero, .mode-1080x960 .grid { gap: 8px; }
    .mode-1080x960 .barTop { grid-template-columns: 1fr 1fr; }
    .mode-1080x960 .statusLine, .mode-1080x960 .sessionMetric, .mode-1080x960 .positionMetric, .mode-1080x960 .trackMetric, .mode-1080x960 .airMetric, .mode-1080x960 .inputPanel { display: none; }
    .mode-1080x960 .metric { padding: 8px 14px; }
    .mode-1080x960 .metric strong, .mode-1080x960 .label, .mode-1080x960 .smallLabel { font-size: 11px; }
    .mode-1080x960 .metric b { font-size: 28px; }
    .mode-1080x960 .hero { grid-template-columns: .82fr 1.48fr; min-height: 0; }
    .mode-1080x960 .speed, .mode-1080x960 .gear, .mode-1080x960 .leds { display: none; }
    .mode-1080x960 .gapHeroPanel, .mode-1080x960 .rpmPanel { min-height: 300px; }
    .mode-1080x960 .gapHeroPanel { display: grid; padding: 12px; gap: 12px; }
    .mode-1080x960 .gapHeroPanel .kv b { font-size: 46px; }
    .mode-1080x960 .rpmPanel { padding: 10px; align-content: stretch; }
    .mode-1080x960 .rpmReadout { display: flex; }
    .mode-1080x960 .speedInline { display: grid; }
    .mode-1080x960 .rpmValue b { font-size: 30px; }
    .mode-1080x960 .rpmValue span { font-size: 18px; }
    .mode-1080x960 .trace { height: 248px; }
    .mode-1080x960 .grid { grid-template-columns: .88fr 1.12fr; min-height: 0; }
    .mode-1080x960 .panel { padding: 10px; gap: 10px; min-height: 0; overflow: hidden; }
    .mode-1080x960 .panel h2 { font-size: 12px; }
    .mode-1080x960 .twoCols, .mode-1080x960 .wheels { gap: 8px; }
    .mode-1080x960 .tyrePanel .track, .mode-1080x960 .tyrePanel .mini, .mode-1080x960 .racePanel .gapMetric, .mode-1080x960 .secondaryMetric, .mode-1080x960 .secondaryGroup, .mode-1080x960 .fuelRow, .mode-1080x960 .damageBars { display: none; }
    .mode-1080x960 .wheel { padding: 10px; gap: 4px; }
    .mode-1080x960 .wheelTop b { font-size: 34px; }
    .mode-1080x960 .kv { gap: 8px; padding-top: 8px; }
    .mode-1080x960 .lapDetail b, .mode-1080x960 .gapMetric b, .mode-1080x960 .coreOnly b { font-size: 25px; }
    .mode-1080x960 .gapMetric { padding-top: 10px; }
    .mode-1080x960 .coreOnly { display: grid; }
    .mode-1080x960 .ersRow { gap: 7px; }
    .mode-1080x960 .ersRow .rowHead b { font-size: 25px; }
    .mode-1080x960 .track { height: 24px; }
  </style>
</head>
<body>
<main id="app" class="stale">
  <section class="barTop">
    <div class="panel statusLine">
      <span class="brand">SIM MOZA BRIDGE</span>
      <span class="pill warn" id="statePill">WAITING</span>
      <span class="notice" id="stateText">NO TELEMETRY</span>
    </div>
    <div class="metric sessionMetric"><strong>SESSION</strong><b id="sessionLeft">--:--</b></div>
    <div class="metric lapMetric"><strong>LAP</strong><b id="lapCount">--/--</b></div>
    <div class="metric positionMetric"><strong>POSITION</strong><b id="position">P--</b></div>
    <div class="metric flagMetric" id="flagMetric"><strong>FLAG</strong><b id="flag">CLEAR</b></div>
    <div class="metric trackMetric"><strong>TRACK</strong><b id="trackTemp">-- C</b></div>
    <div class="metric airMetric"><strong>AIR</strong><b id="airTemp">-- C</b></div>
  </section>

  <section class="hero">
    <div class="panel gapHeroPanel coreOnly">
      <div class="kv gapMetric"><span class="smallLabel">GAP AHEAD</span><b id="gapFrontHero">--</b></div>
      <div class="kv gapMetric"><span class="smallLabel">GAP BEHIND</span><b id="gapBehindHero">--</b></div>
    </div>
    <div class="speed"><span>SPEED</span><b id="speed">0</b><span>KM/H</span></div>
    <div class="gear"><b id="gear">N</b></div>
    <div class="rpmPanel">
      <div class="leds" id="leds"></div>
      <div class="rpmReadout">
        <div class="speedInline"><span>SPEED</span><b id="speedInline">0</b><span>KM/H</span></div>
        <div class="rpmValue"><b id="rpm">0 RPM</b><span id="rev">0%</span></div>
      </div>
      <canvas class="trace" id="trace" width="1000" height="180"></canvas>
    </div>
  </section>

  <section class="grid">
    <div class="panel inputPanel">
      <h2>Inputs</h2>
      <div class="inputRows">
        <div class="row">
          <div class="rowHead"><span class="label">THROTTLE</span><b id="throttleValue">0%</b></div>
          <div class="track"><div class="fill" id="throttle"></div></div>
        </div>
        <div class="row">
          <div class="rowHead"><span class="label">BRAKE</span><b id="brakeValue">0%</b></div>
          <div class="track"><div class="fill" id="brake"></div></div>
        </div>
        <div class="row">
          <div class="rowHead"><span class="label">STEER</span><b id="steerValue">0%</b></div>
          <div class="steerWrap">
            <div class="steerCenter"></div>
            <div class="steerFill" id="steerLeft"></div>
            <div class="steerFill" id="steerRight"></div>
          </div>
        </div>
        <div class="row">
          <div class="rowHead"><span class="label">CLUTCH</span><b id="clutchValue">0%</b></div>
          <div class="track"><div class="fill" id="clutchBar"></div></div>
        </div>
      </div>
      <div class="twoCols">
        <div class="kv"><span class="smallLabel">DRS</span><b id="drs">OFF</b></div>
        <div class="kv"><span class="smallLabel">DRS DIST</span><b id="drsDistance">-- m</b></div>
        <div class="kv"><span class="smallLabel">PIT LIMIT</span><b id="pitLimiter">OFF</b></div>
        <div class="kv"><span class="smallLabel">FRAME</span><b id="frame">0</b></div>
      </div>
    </div>

    <div class="panel tyrePanel">
      <h2>Tyres & Brakes</h2>
      <div class="wheels">
        <div class="wheel" data-corner="fl">
          <div class="wheelTop"><span class="label">FL</span><b data-field="wear">--%</b></div>
          <div class="track"><div class="fill" data-field="wearBar"></div></div>
          <div class="mini">
            <div><span class="smallLabel">SURF</span><b data-field="surface">-- C</b></div>
            <div><span class="smallLabel">INNER</span><b data-field="inner">-- C</b></div>
            <div><span class="smallLabel">PRESS</span><b data-field="pressure">-- PSI</b></div>
            <div><span class="smallLabel">BRAKE</span><b data-field="brakeTemp">-- C</b></div>
            <div><span class="smallLabel">DAMAGE</span><b data-field="damage">--%</b></div>
            <div><span class="smallLabel">BLISTER</span><b data-field="blister">--%</b></div>
          </div>
        </div>
        <div class="wheel" data-corner="fr">
          <div class="wheelTop"><span class="label">FR</span><b data-field="wear">--%</b></div>
          <div class="track"><div class="fill" data-field="wearBar"></div></div>
          <div class="mini">
            <div><span class="smallLabel">SURF</span><b data-field="surface">-- C</b></div>
            <div><span class="smallLabel">INNER</span><b data-field="inner">-- C</b></div>
            <div><span class="smallLabel">PRESS</span><b data-field="pressure">-- PSI</b></div>
            <div><span class="smallLabel">BRAKE</span><b data-field="brakeTemp">-- C</b></div>
            <div><span class="smallLabel">DAMAGE</span><b data-field="damage">--%</b></div>
            <div><span class="smallLabel">BLISTER</span><b data-field="blister">--%</b></div>
          </div>
        </div>
        <div class="wheel" data-corner="rl">
          <div class="wheelTop"><span class="label">RL</span><b data-field="wear">--%</b></div>
          <div class="track"><div class="fill" data-field="wearBar"></div></div>
          <div class="mini">
            <div><span class="smallLabel">SURF</span><b data-field="surface">-- C</b></div>
            <div><span class="smallLabel">INNER</span><b data-field="inner">-- C</b></div>
            <div><span class="smallLabel">PRESS</span><b data-field="pressure">-- PSI</b></div>
            <div><span class="smallLabel">BRAKE</span><b data-field="brakeTemp">-- C</b></div>
            <div><span class="smallLabel">DAMAGE</span><b data-field="damage">--%</b></div>
            <div><span class="smallLabel">BLISTER</span><b data-field="blister">--%</b></div>
          </div>
        </div>
        <div class="wheel" data-corner="rr">
          <div class="wheelTop"><span class="label">RR</span><b data-field="wear">--%</b></div>
          <div class="track"><div class="fill" data-field="wearBar"></div></div>
          <div class="mini">
            <div><span class="smallLabel">SURF</span><b data-field="surface">-- C</b></div>
            <div><span class="smallLabel">INNER</span><b data-field="inner">-- C</b></div>
            <div><span class="smallLabel">PRESS</span><b data-field="pressure">-- PSI</b></div>
            <div><span class="smallLabel">BRAKE</span><b data-field="brakeTemp">-- C</b></div>
            <div><span class="smallLabel">DAMAGE</span><b data-field="damage">--%</b></div>
            <div><span class="smallLabel">BLISTER</span><b data-field="blister">--%</b></div>
          </div>
        </div>
      </div>
    </div>

    <div class="panel racePanel">
      <h2>Race & Systems</h2>
      <div class="twoCols">
        <div class="kv lapDetail"><span class="smallLabel">CURRENT</span><b id="currentLap">--:--.---</b></div>
        <div class="kv lapDetail"><span class="smallLabel">LAST</span><b id="lastLap">--:--.---</b></div>
        <div class="kv secondaryMetric sectorMetric" id="sector1Metric"><span class="smallLabel">SECTOR 1</span><b id="sector1">--:--.---</b></div>
        <div class="kv secondaryMetric sectorMetric" id="sector2Metric"><span class="smallLabel">SECTOR 2</span><b id="sector2">--:--.---</b></div>
        <div class="kv secondaryMetric sectorMetric" id="sector3Metric"><span class="smallLabel">SECTOR 3</span><b id="sector3">--:--.---</b></div>
        <div class="kv gapMetric"><span class="smallLabel">GAP AHEAD</span><b id="gapFront">--</b></div>
        <div class="kv gapMetric"><span class="smallLabel">GAP BEHIND</span><b id="gapBehind">--</b></div>
        <div class="kv gapMetric"><span class="smallLabel">GAP LEADER</span><b id="gapLeader">--</b></div>
        <div class="kv coreOnly"><span class="smallLabel">DRS</span><b id="drsCore">OFF</b></div>
        <div class="kv secondaryMetric"><span class="smallLabel">FUEL LAPS</span><b id="fuelLaps">--</b></div>
        <div class="kv secondaryMetric"><span class="smallLabel">BRAKE BIAS</span><b id="brakeBias">--%</b></div>
      </div>
      <div class="row fuelRow">
        <div class="rowHead"><span class="label">FUEL</span><b id="fuelValue">-- L</b></div>
        <div class="track"><div class="fill" id="fuelBar"></div></div>
      </div>
      <div class="row ersRow">
        <div class="rowHead"><span class="label">ERS</span><b id="ersValue">--%</b></div>
        <div class="track"><div class="fill" id="ersBar"></div></div>
      </div>
      <div class="twoCols secondaryGroup">
        <div class="kv"><span class="smallLabel">TYRE</span><b id="compound">--</b></div>
        <div class="kv"><span class="smallLabel">TYRE AGE</span><b id="tyreAge">--</b></div>
        <div class="kv"><span class="smallLabel">TC</span><b id="tractionControl">--</b></div>
        <div class="kv"><span class="smallLabel">ABS</span><b id="abs">--</b></div>
      </div>
      <div class="damageBars">
        <div class="damageBar"><div class="damageTrack"><div class="damageFill" id="wingLf"></div></div><span>WING L</span><b id="wingLfValue">--</b></div>
        <div class="damageBar"><div class="damageTrack"><div class="damageFill" id="wingRf"></div></div><span>WING R</span><b id="wingRfValue">--</b></div>
        <div class="damageBar"><div class="damageTrack"><div class="damageFill" id="wingRear"></div></div><span>REAR</span><b id="wingRearValue">--</b></div>
        <div class="damageBar"><div class="damageTrack"><div class="damageFill" id="engineDamage"></div></div><span>ENGINE</span><b id="engineDamageValue">--</b></div>
        <div class="damageBar"><div class="damageTrack"><div class="damageFill" id="gearboxDamage"></div></div><span>GEARBOX</span><b id="gearboxDamageValue">--</b></div>
      </div>
    </div>
  </section>
</main>

<script>
const clamp = (value, min, max) => Math.max(min, Math.min(max, value));
const pct = value => clamp(Math.round((value || 0) * 100), 0, 100);
const percentValue = value => value === null || value === undefined || value < 0 ? '--%' : clamp(Math.round(value), 0, 100) + '%';
const numberValue = (value, suffix = '', digits = 0) => value === null || value === undefined ? '--' + suffix : Number(value).toFixed(digits) + suffix;
const intValue = (value, suffix = '') => value === null || value === undefined ? '--' + suffix : Math.round(value) + suffix;
const setText = (id, value) => { document.getElementById(id).textContent = value; };
const setWidth = (id, value) => { document.getElementById(id).style.width = clamp(value || 0, 0, 100) + '%'; };
const setHeight = (id, value) => { document.getElementById(id).style.height = clamp(value || 0, 0, 100) + '%'; };
const gearLabel = value => value === -1 ? 'R' : value === 0 ? 'N' : value === null || value === undefined ? 'N' : String(value);
const tcLabel = value => value === 0 ? 'OFF' : value === 1 ? 'MED' : value === 2 ? 'FULL' : '--';
const absLabel = value => value === 0 ? 'OFF' : value === 1 ? 'ON' : '--';
const compoundLabel = value => ({ 7: 'INT', 8: 'WET', 16: 'SOFT', 17: 'MED', 18: 'HARD' }[value] || (value === null || value === undefined ? '--' : String(value)));
function timeMs(value) {
  if (value === null || value === undefined) return '--:--.---';
  const minutes = Math.floor(value / 60000);
  const seconds = Math.floor((value % 60000) / 1000);
  const ms = Math.floor(value % 1000);
  return minutes + ':' + String(seconds).padStart(2, '0') + '.' + String(ms).padStart(3, '0');
}
function timeSeconds(value) {
  if (value === null || value === undefined) return '--:--';
  const minutes = Math.floor(value / 60);
  const seconds = Math.floor(value % 60);
  return minutes + ':' + String(seconds).padStart(2, '0');
}
function gap(value) {
  if (value === null || value === undefined) return '--';
  return '+' + (value / 1000).toFixed(3);
}
const flagMeta = flag => ({
  '-1': ['UNKNOWN', 'neutral'],
  0: ['CLEAR', 'neutral'],
  1: ['GREEN', 'green'],
  2: ['BLUE', 'blue'],
  3: ['YELLOW', 'yellow'],
  4: ['RED', 'red'],
}[flag] || ['CLEAR', 'neutral']);
function setStateClass(element, prefix, value) {
  Array.from(element.classList)
    .filter(className => className.startsWith(prefix))
    .forEach(className => element.classList.remove(className));
  if (value && value !== 'neutral') element.classList.add(prefix + value);
}
function currentFlag(session, lap) {
  const zones = Array.isArray(session.marshalZones) ? session.marshalZones : [];
  const trackLength = session.trackLengthM || 0;
  if (zones.length === 0 || trackLength <= 0 || lap.lapDistanceM === null || lap.lapDistanceM === undefined) return 0;
  const lapDistance = ((lap.lapDistanceM % trackLength) + trackLength) % trackLength;
  const fraction = lapDistance / trackLength;
  const sorted = zones
    .filter(zone => Number.isFinite(zone.start) && zone.start >= 0 && zone.start <= 1)
    .sort((left, right) => left.start - right.start);
  let active = sorted[sorted.length - 1];
  for (const zone of sorted) {
    if (zone.start <= fraction) active = zone; else break;
  }
  return active ? active.flag : 0;
}
function setFlag(flag) {
  const [label, className] = flagMeta(flag);
  setText('flag', label);
  setStateClass(document.getElementById('flagMetric'), 'flag-', className);
}
const sectorBest = { 1: null, 2: null, 3: null };
function sector3Time(lap) {
  const last = lap.lastLapTimeMs;
  const first = lap.sector1TimeMs;
  const second = lap.sector2TimeMs;
  if (last === null || last === undefined || first === null || first === undefined || second === null || second === undefined) return null;
  const third = last - first - second;
  return third > 0 ? third : null;
}
function sectorClass(index, value, currentSector) {
  if (currentSector === index - 1 && (value === null || value === undefined)) return 'yellow';
  if (value === null || value === undefined) return 'neutral';
  const best = sectorBest[index];
  if (best === null || value < best) {
    sectorBest[index] = value;
    return 'purple';
  }
  return value === sectorBest[index] ? 'purple' : 'green';
}
function setSector(index, value, currentSector) {
  setText('sector' + index, timeMs(value));
  setStateClass(document.getElementById('sector' + index + 'Metric'), 'sector-', sectorClass(index, value, currentSector));
}
function updateLayoutMode() {
  const app = document.getElementById('app');
  const width = window.innerWidth;
  const height = window.innerHeight;
  const ratio = width / Math.max(1, height);
  app.classList.remove('mode-1920x1080', 'mode-1080x1920', 'mode-1080x960');
  if (ratio >= 1.55) {
    app.classList.add('mode-1920x1080');
  } else if (ratio <= 0.72) {
    app.classList.add('mode-1080x1920');
  } else if (width >= 900 && height <= 1120) {
    app.classList.add('mode-1080x960');
  }
}
window.addEventListener('resize', updateLayoutMode);
window.addEventListener('orientationchange', updateLayoutMode);
updateLayoutMode();

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
  drawFlagBands(width, height);
  ctx.strokeStyle = '#3a3528';
  ctx.lineWidth = 1;
  for (let i = 1; i < 4; i += 1) {
    const y = i * height / 4;
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(width, y);
    ctx.stroke();
  }
  drawLine('throttle', '#21d17c', value => height - value * height);
  drawLine('brake', '#ff4d45', value => height - value * height);
  drawLine('steer', '#51d6d0', value => height / 2 - value * height / 2);
  drawAssistTicks('tc', '#b58cff', 0, 16);
  drawAssistTicks('abs', '#f0c247', height - 16, height);
}
function drawFlagBands(width, height) {
  if (trace.length < 2) return;
  trace.forEach((sample, index) => {
    const color = { 1: 'rgba(33, 209, 124, .12)', 2: 'rgba(81, 214, 208, .14)', 3: 'rgba(240, 194, 71, .16)', 4: 'rgba(255, 77, 69, .18)' }[sample.flag];
    if (!color) return;
    const x = index * width / Math.max(1, trace.length - 1);
    ctx.fillStyle = color;
    ctx.fillRect(x, 0, Math.max(2, width / Math.max(1, trace.length)), height);
  });
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
function drawAssistTicks(key, color, y1, y2) {
  if (trace.length < 2) return;
  ctx.strokeStyle = color;
  ctx.lineWidth = 2;
  trace.forEach((sample, index) => {
    if (!sample[key]) return;
    const x = index * canvas.width / Math.max(1, trace.length - 1);
    ctx.beginPath();
    ctx.moveTo(x, y1);
    ctx.lineTo(x, y2);
    ctx.stroke();
  });
}

function updateWheel(corner, state) {
  const root = document.querySelector(`[data-corner="${corner}"]`);
  const input = state.input || {};
  const damage = state.damage || {};
  const tyreWear = damage.tyreWear || {};
  const tyreDamage = damage.tyreDamage || {};
  const tyreBlisters = damage.tyreBlisters || {};
  const surface = input.tyreSurfaceTempsC || {};
  const inner = input.tyreInnerTempsC || {};
  const pressure = input.tyrePressuresPsi || {};
  const brakeTemp = input.brakeTempsC || {};
  const wear = tyreWear[corner];
  root.querySelector('[data-field="wear"]').textContent = percentValue(wear);
  root.querySelector('[data-field="wearBar"]').style.width = clamp(wear || 0, 0, 100) + '%';
  root.querySelector('[data-field="wearBar"]').style.background = wear >= 65 ? '#ff4d45' : wear >= 35 ? '#f0c247' : '#21d17c';
  root.querySelector('[data-field="surface"]').textContent = intValue(surface[corner], ' C');
  root.querySelector('[data-field="inner"]').textContent = intValue(inner[corner], ' C');
  root.querySelector('[data-field="pressure"]').textContent = numberValue(pressure[corner], ' PSI', 1);
  root.querySelector('[data-field="brakeTemp"]').textContent = intValue(brakeTemp[corner], ' C');
  root.querySelector('[data-field="damage"]').textContent = percentValue(tyreDamage[corner]);
  root.querySelector('[data-field="blister"]').textContent = percentValue(tyreBlisters[corner]);
}

function updateDamage(id, value) {
  setHeight(id, value || 0);
  setText(id + 'Value', percentValue(value));
}

function render(state) {
  const input = state.input || {};
  const lap = state.lap || {};
  const session = state.session || {};
  const status = state.status || {};
  const damage = state.damage || {};
  const live = Boolean(state.input);
  document.getElementById('app').classList.toggle('stale', !live);
  const pill = document.getElementById('statePill');
  pill.textContent = live ? 'LIVE' : 'WAITING';
  pill.classList.toggle('live', live);
  pill.classList.toggle('warn', !live);
  setText('stateText', live ? 'F1 25 UDP' : 'NO TELEMETRY');

  const throttle = pct(input.throttle);
  const brake = pct(input.brake);
  const steer = clamp(Math.round((input.steer || 0) * 100), -100, 100);
  const clutch = clamp(Math.round(input.clutch || 0), 0, 100);
  const rev = clamp(input.revLightsPercent || 0, 0, 100);
  const activeFlag = currentFlag(session, lap);
  setText('speed', input.speedKmh ?? 0);
  setText('speedInline', input.speedKmh ?? 0);
  setText('gear', gearLabel(input.gear));
  setText('rpm', (input.rpm ?? 0) + ' RPM');
  setText('rev', Math.round(rev) + '%');
  setText('throttleValue', throttle + '%');
  setText('brakeValue', brake + '%');
  setText('steerValue', steer + '%');
  setText('clutchValue', clutch + '%');
  setWidth('throttle', throttle);
  setWidth('brake', brake);
  setWidth('clutchBar', clutch);
  document.getElementById('steerLeft').style.left = (50 - Math.max(0, -steer) / 2) + '%';
  document.getElementById('steerLeft').style.width = Math.max(0, -steer) / 2 + '%';
  document.getElementById('steerRight').style.left = '50%';
  document.getElementById('steerRight').style.width = Math.max(0, steer) / 2 + '%';
  Array.from(leds.children).forEach((led, index) => {
    led.classList.toggle('on', index < Math.ceil(rev / 100 * 15));
    led.classList.toggle('flash', rev >= 95);
  });

  setText('frame', input.frameIdentifier ?? 0);
  setText('drs', input.drs ? 'ON' : 'OFF');
  setText('drsCore', input.drs ? 'ON' : 'OFF');
  setText('drsDistance', status.drsActivationDistanceM === 0 || status.drsActivationDistanceM ? status.drsActivationDistanceM + ' m' : '-- m');
  setText('pitLimiter', status.pitLimiterActive ? 'ON' : 'OFF');
  setText('sessionLeft', timeSeconds(session.sessionTimeLeftS));
  setText('lapCount', (lap.currentLapNum || '--') + '/' + (session.totalLaps || '--'));
  setText('position', lap.carPosition ? 'P' + lap.carPosition : 'P--');
  setText('trackTemp', intValue(session.trackTempC, ' C'));
  setText('airTemp', intValue(session.airTempC, ' C'));
  setFlag(activeFlag);

  setText('currentLap', timeMs(lap.currentLapTimeMs));
  setText('lastLap', timeMs(lap.lastLapTimeMs));
  setSector(1, lap.sector1TimeMs, lap.sector);
  setSector(2, lap.sector2TimeMs, lap.sector);
  setSector(3, sector3Time(lap), lap.sector);
  const gapFrontValue = gap(lap.deltaToCarInFrontMs);
  const gapBehindValue = gap(lap.deltaToCarBehindMs);
  setText('gapFront', gapFrontValue);
  setText('gapBehind', gapBehindValue);
  setText('gapFrontHero', gapFrontValue);
  setText('gapBehindHero', gapBehindValue);
  setText('gapLeader', gap(lap.deltaToRaceLeaderMs));
  setText('fuelLaps', numberValue(status.fuelRemainingLaps, '', 1));
  setText('brakeBias', intValue(status.frontBrakeBias, '%'));
  setText('fuelValue', numberValue(status.fuelInTank, ' L', 1));
  setWidth('fuelBar', status.fuelCapacity ? status.fuelInTank / status.fuelCapacity * 100 : 0);
  setText('ersValue', percentValue(status.ersPercent));
  setWidth('ersBar', status.ersPercent || 0);
  setText('compound', compoundLabel(status.visualTyreCompound));
  setText('tyreAge', status.tyresAgeLaps === 0 || status.tyresAgeLaps ? status.tyresAgeLaps + ' L' : '--');
  setText('tractionControl', tcLabel(status.tractionControl));
  setText('abs', absLabel(status.antiLockBrakes));

  ['fl', 'fr', 'rl', 'rr'].forEach(corner => updateWheel(corner, state));
  updateDamage('wingLf', damage.frontLeftWingDamage);
  updateDamage('wingRf', damage.frontRightWingDamage);
  updateDamage('wingRear', damage.rearWingDamage);
  updateDamage('engineDamage', damage.engineDamage);
  updateDamage('gearboxDamage', damage.gearboxDamage);

  trace.push({
    throttle: input.throttle || 0,
    brake: input.brake || 0,
    steer: input.steer || 0,
    tc: (status.tractionControl || 0) > 0 && throttle >= 70,
    abs: (status.antiLockBrakes || 0) > 0 && brake >= 55,
    flag: activeFlag,
  });
  if (trace.length > 220) trace.shift();
  drawTrace();
}

async function tick() {
  try {
    const state = await fetch('/state', { cache: 'no-store' }).then(response => response.json());
    render(state);
  } catch (_) {}
  setTimeout(() => requestAnimationFrame(tick), 16);
}
requestAnimationFrame(tick);
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_state_merges_partial_updates() {
        let mut state = HudState::default();
        let input = InputSample {
            session_time: 1.0,
            frame_identifier: 7,
            player_car_index: 0,
            throttle: 0.5,
            steer: -0.25,
            brake: 0.0,
            clutch: 3,
            speed_kmh: 210,
            gear: 5,
            rpm: 11_500,
            drs: true,
            rev_lights_percent: 90,
            rev_lights_bit_value: 0,
            brake_temps_c: WheelValuesU16 {
                fl: 600,
                fr: 610,
                rl: 500,
                rr: 510,
            },
            tyre_surface_temps_c: WheelValuesU8 {
                fl: 91,
                fr: 92,
                rl: 88,
                rr: 89,
            },
            tyre_inner_temps_c: WheelValuesU8 {
                fl: 94,
                fr: 95,
                rl: 90,
                rr: 91,
            },
            engine_temp_c: 105,
            tyre_pressures_psi: WheelValuesF32 {
                fl: 23.1,
                fr: 23.2,
                rl: 21.1,
                rr: 21.2,
            },
        };
        let damage = DamageSample {
            session_time: 1.1,
            frame_identifier: 8,
            player_car_index: 0,
            tyre_wear: WheelValuesF32 {
                fl: 10.0,
                fr: 11.0,
                rl: 12.0,
                rr: 13.0,
            },
            tyre_damage: WheelValuesU8 {
                fl: 1,
                fr: 2,
                rl: 3,
                rr: 4,
            },
            tyre_blisters: WheelValuesU8 {
                fl: 5,
                fr: 6,
                rl: 7,
                rr: 8,
            },
            front_left_wing_damage: 9,
            front_right_wing_damage: 10,
            rear_wing_damage: 11,
            gearbox_damage: 12,
            engine_damage: 13,
        };

        state.apply(&TelemetryUpdate {
            input: Some(input),
            ..TelemetryUpdate::default()
        });
        state.apply(&TelemetryUpdate {
            damage: Some(damage),
            ..TelemetryUpdate::default()
        });

        let json = state.to_json();
        assert!(json.contains("\"speedKmh\":210"));
        assert!(json.contains("\"tyreWear\":{\"fl\":10.000"));
        assert!(json.contains("\"status\":null"));
    }
}
