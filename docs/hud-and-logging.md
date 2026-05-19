# HUD and Input Logging

The Rust bridge can extract F1 25 player input samples from `PacketCarTelemetryData`.

Current fields:

- `session_time`
- `frame_identifier`
- `player_car_index`
- `throttle`
- `brake`
- `speed_kmh`
- `gear`
- `rpm`

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

The HUD polls `/state` at roughly 60Hz and renders throttle/brake bars plus speed, gear, RPM, and frame.

## Scope

This is not a full SimHub clone. It is the first local display path for live input telemetry. The next natural steps are:

- Add steering and clutch.
- Add session/lap/delta data.
- Add a WebSocket or Server-Sent Events transport instead of polling.
- Add reusable dashboard layout files.
