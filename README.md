# F1 MOZA Bridge

Experimental game-profile telemetry bridge for forwarding sim racing telemetry to MOZA Pit House and testing packet-level remaps for MOZA dashboards.

The first fully supported adapter is F1 25 over UDP. Its first remap fixes a common shape mismatch: F1 25 wheel arrays use `RL, RR, FL, FR`, while MOZA dashboard field names are exposed as `FL, FR, RL, RR`. The bridge can rewrite the tyre wear array before forwarding packets, which is useful when a downstream consumer treats the raw array as front-left first.

This is not an official MOZA or EA tool.

## Game Profiles

| Game | Profile | Current support | Notes |
| --- | --- | --- | --- |
| F1 25 | `f1-25` | UDP passthrough + F1 packet remaps | Default profile |
| Generic UDP | `generic-udp` | UDP passthrough only | Use when another tool already exports compatible UDP |
| Assetto Corsa EVO | `ace` | Documented, adapter pending | Public integrations point to shared memory/helper-server access, not simple UDP |
| Le Mans Ultimate | `lmu` | Documented, adapter pending | MOZA uses the rF2 shared-memory plugin path |

ACE and LMU are intentionally listed even though the bridge cannot read them directly yet. They need different input adapters, not just another UDP port.

## Requirements

- Windows PC running the target sim and MOZA Pit House
- Node.js 24 or newer
- UDP telemetry enabled for UDP-backed profiles

## F1 25 Settings

Set F1 25 telemetry to send packets to the bridge:

| Setting | Value |
| --- | --- |
| UDP Telemetry | On |
| UDP IP Address | `127.0.0.1` |
| UDP Port | `20777` |
| UDP Send Rate | `20Hz` or `40Hz` |
| UDP Format | `2025` |

MOZA Pit House normally listens for F1 25 on port `22025`, so the bridge forwards there by default.

## Usage

Passthrough mode sends packets unchanged. Use this first to prove F1 25, the bridge, and Pit House are connected.

```bash
npm start -- --game f1-25 --listen 20777 --moza-port 22025 --mode passthrough
```

Tyre wear remap mode rewrites `Car Damage` packet tyre wear arrays from F1 wheel order to dashboard-friendly order before forwarding.

```bash
npm start -- --game f1-25 --listen 20777 --moza-port 22025 --mode remap --fix-tyre-wear-order
```

Verbose mode prints packet counts and patched packet counts once per second.

```bash
npm start -- --game f1-25 --mode remap --fix-tyre-wear-order --verbose
```

Dry run mode parses and patches packets but does not forward them.

```bash
npm start -- --game f1-25 --mode remap --fix-tyre-wear-order --dry-run
```

Generic UDP passthrough can forward packets from any external exporter to any target UDP port. The bridge does not inspect these packets.

```bash
npm start -- --game generic-udp --listen 20777 --moza-port 22025 --mode passthrough
```

## Why This Exists

F1 25 UDP telemetry is a binary protocol. Wheel arrays in the official F1 25 specification are ordered:

```text
0 = RL
1 = RR
2 = FL
3 = FR
```

MOZA dashboard fields are named:

```text
TyreWearFL
TyreWearFR
TyreWearRL
TyreWearRR
```

If MOZA Pit House already maps the F1 25 array correctly, do not enable the remap. If the displayed tyre wear values appear swapped, enable `--fix-tyre-wear-order` and verify with clearly asymmetric tyre wear.

## MOZA Dashboard Binding

MOZA Dash Studio bindings use JavaScript expressions such as:

```js
Telemetry.get("v1/gameData/Rpm").value
```

Most dashboard telemetry values follow this shape:

```text
v1/gameData/<TelemetryName>
```

Useful examples for an F1-style dashboard:

| Display | Binding |
| --- | --- |
| Gear | `Telemetry.get("v1/gameData/Gear").value` |
| RPM | `Telemetry.get("v1/gameData/Rpm").value` |
| RPM percent | `Telemetry.get("v1/gameData/CarSettings_CurrentDisplayedRPMPercent").value` |
| Speed | `Telemetry.get("v1/gameData/SpeedKmh").value` |
| DRS | `Telemetry.get("v1/gameData/Drs").value` |
| DRS available | `Telemetry.get("v1/gameData/DRSAvailable").value` |
| DRS allowed | `Telemetry.get("v1/gameData/DRSAllowed").value` |
| ERS percent | `Telemetry.get("v1/gameData/ERSPercent").value` |
| ERS stored | `Telemetry.get("v1/gameData/ERSStored").value` |
| Fuel laps | `Telemetry.get("v1/gameData/FuelRemainLaps").value` |
| Brake bias | `Telemetry.get("v1/gameData/BrakeBias").value` |
| Front-left tyre wear | `Telemetry.get("v1/gameData/TyreWearFL").value` |
| Front-right tyre wear | `Telemetry.get("v1/gameData/TyreWearFR").value` |
| Rear-left tyre wear | `Telemetry.get("v1/gameData/TyreWearRL").value` |
| Rear-right tyre wear | `Telemetry.get("v1/gameData/TyreWearRR").value` |

The bridge does not register new MOZA keys. For example, it cannot create `v1/gameData/BehindGap` unless Pit House already exposes that key. It can only change the F1 UDP packets that Pit House reads underneath existing keys.

## Command Options

| Option | Default | Description |
| --- | --- | --- |
| `--game` | `f1-25` | Game profile: `f1-25`, `generic-udp`, `ace`, `lmu` |
| `--listen` | Profile default | UDP port receiving game packets |
| `--listen-host` | `0.0.0.0` | Host/interface to bind |
| `--moza-host` | `127.0.0.1` | MOZA Pit House host |
| `--moza-port` | Profile default | MOZA Pit House target telemetry port |
| `--mode` | `passthrough` | `passthrough` or `remap` |
| `--fix-tyre-wear-order` | `false` | Rewrite F1 25 `m_tyresWear[4]` order |
| `--dry-run` | `false` | Do not forward packets |
| `--verbose` | `false` | Print runtime stats |

## Current Scope

Implemented:

- F1 25 packet header parsing
- UDP passthrough to MOZA Pit House
- Experimental tyre wear order remap for `PacketCarDamageData`
- `--game` profile selection with guarded ACE/LMU placeholders
- Packet-level tests for header parsing and tyre wear remap offsets

Not implemented yet:

- Native ACE shared-memory adapter
- Native LMU/rFactor shared-memory adapter
- Behind gap injection into MOZA dashboards
- F1 25 to F1 24 packet down-conversion
- A graphical UI
- Direct Mission R OLED rendering

## Safety Notes

The bridge never sends input back to F1 25. It only listens for UDP telemetry and forwards UDP telemetry to Pit House.

If Pit House telemetry stops, close F1 25, Pit House, and the bridge, then start them again in this order:

1. MOZA Pit House
2. F1 MOZA Bridge
3. The target game
