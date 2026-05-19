# Game Profiles

The bridge is now organized around game profiles. A profile describes the input source, protocol parser, default ports, and supported remaps.

## Current Profiles

| Profile | Game | Input | Bridge status |
| --- | --- | --- | --- |
| `auto` | Packet-based detection | UDP packets | Supported for F1 25 detection |
| `f1-25` | F1 25 | UDP binary packets | Supported |
| `generic-udp` | Any external UDP exporter | UDP packets | Passthrough only |
| `ace` | Assetto Corsa EVO | Shared memory / helper-server style integrations | Adapter pending |
| `lmu` | Le Mans Ultimate | rFactor/LMU shared memory plugin path | Adapter pending |

## Detection Boundary

`--game auto` inspects incoming telemetry packets. It is not a process scanner.

Supported today:

- F1 25 packet header -> selects `f1-25`
- Unknown UDP packet -> forwards as raw UDP for that packet and keeps waiting for a recognizable packet

Not supported yet:

- Detecting ACE from the Windows process list
- Detecting LMU from the Windows process list
- Reading shared-memory telemetry automatically

Process detection can be added later for UI convenience, but it is not enough by itself. The bridge needs the actual telemetry protocol to parse or transform data safely.

## Why ACE and LMU Are Different

F1 25 exposes a documented UDP protocol, so the bridge can sit directly between the game and MOZA Pit House:

```text
F1 25 UDP -> bridge -> MOZA Pit House
```

Assetto Corsa EVO and Le Mans Ultimate do not currently fit that simple model.

Assetto Corsa EVO public integrations point toward shared-memory or helper-server access. That needs a Windows shared-memory reader or a separate exporter before this bridge can normalize and forward anything.

Le Mans Ultimate uses the rFactor-style shared-memory plugin route for MOZA Pit House. MOZA's setup copies `rF2SharedMemoryMapPlugin64.dll` into:

```text
Le Mans Ultimate/Bin64/Plugins/
```

That means a native LMU adapter should read from the LMU/rFactor shared-memory API, not listen for ordinary game UDP.

## Roadmap

The planned architecture is:

```text
game source
  -> input adapter
  -> normalized telemetry model
  -> output adapter
  -> MOZA Pit House or another dashboard
```

Input adapters:

- `f1-25-udp`: implemented
- `generic-udp`: implemented as opaque passthrough
- `ace-shared-memory`: planned
- `lmu-shared-memory`: planned

Output adapters:

- `moza-udp`: implemented for packets that Pit House already understands
- `web-dashboard`: planned for values MOZA does not expose as `v1/gameData/...`

## Practical Use Today

For F1 25:

```bash
cargo run -- --mode remap --fix-tyre-wear-order
```

For an external exporter that already emits compatible packets:

```bash
cargo run -- --game generic-udp --listen 20777 --moza-port 22025
```

For ACE or LMU, running `--game ace` or `--game lmu` will currently fail with a clear explanation instead of pretending a UDP bridge is enough.
