# HUD and Input Logging

The Rust bridge can extract F1 25 player input samples from `PacketCarTelemetryData` and combine them with lap/session/status packets for local analysis.

Current fields:

- `session_time`
- `frame_identifier`
- `player_car_index`
- `throttle`
- `brake`
- `steer`
- `clutch`
- `speed_kmh`
- `gear`
- `rpm`
- `drs`
- `rev_lights_percent`
- brake temperatures
- tyre temperatures
- tyre pressures

## CSV Logging

```bash
cargo run -- --input-log inputs.csv
```

Rows are appended. A header is written when the file is new or empty.

## Browser HUD

```bash
cargo run -- --hud-http 8765
```

Open:

```text
http://127.0.0.1:8765
```

The HUD polls `/state` at roughly 60Hz and renders throttle/brake/steering bars, speed, gear, RPM, DRS, REV LEDs, frame, and a rolling input trace.

## Lap Analysis

```bash
cargo run -- --corner-log corners.csv --analysis-report analysis.md
```

`--corner-log` writes completed-lap segment summaries. Each lap is split into 20 equal distance buckets using the F1 session track length when available.

`--analysis-report` writes a Markdown snapshot for the latest completed lap. It includes:

- clean lap status
- lap time and sample count
- current fuel, brake bias, ERS, and tyre age when `PacketCarStatusData` is available
- tyre wear when `PacketCarDamageData` is available
- segment trace table
- setup candidates from input trace, tyre wear, tyre temperature, and status heuristics

## Scope

This is not a full SimHub clone. The local HUD and analysis path is intended for quick driver feedback and repeatable CSV/Markdown output.

- Add a WebSocket or Server-Sent Events transport instead of polling.
- Add reusable dashboard layout files.
- Add native ACE and LMU input adapters.
