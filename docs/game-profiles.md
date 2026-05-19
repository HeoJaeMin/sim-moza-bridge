# Game Profiles

The bridge is now organized around game profiles. A profile describes the input source, protocol parser, default ports, and supported remaps.

## Current Profiles

| Profile | Game | Input | Bridge status |
| --- | --- | --- | --- |
| `auto` | Packet-based detection | UDP packets | Supported for F1 25 detection |
| `f1-25` | F1 25 | UDP binary packets | Supported |
| `generic-udp` | Any external UDP exporter | UDP packets | Passthrough only |
| `ace` | Assetto Corsa EVO | Shared memory | Adapter pending |
| `acr` | Assetto Corsa Rally | Shared memory / helper-reader style integrations | Adapter pending |
| `lmu` / `lu` | Le Mans Ultimate | Shared memory / plugin-backed telemetry | Adapter pending |

## Detection Boundary

`--game auto` inspects incoming telemetry packets. It is not a process scanner.

Supported today:

- F1 25 packet header -> selects `f1-25`
- Unknown UDP packet -> forwards as raw UDP for that packet and keeps waiting for a recognizable packet

Not supported yet:

- Detecting ACE from the Windows process list
- Detecting ACR from the Windows process list
- Detecting LMU/LU from the Windows process list
- Reading shared-memory telemetry automatically

Process detection can be added later for UI convenience, but it is not enough by itself. The bridge needs the actual telemetry protocol to parse or transform data safely.

## Why ACE, ACR, and LMU Are Different

F1 25 exposes a documented UDP protocol, so the bridge can sit directly between the game and MOZA Pit House:

```text
F1 25 UDP -> bridge -> MOZA Pit House
```

Assetto Corsa EVO, Assetto Corsa Rally, and Le Mans Ultimate do not currently fit that simple model.

Assetto Corsa EVO now has an updated shared-memory library and official MoTeC support according to the 0.6 release notes. Public dashboard integrations also treat it as local telemetry rather than UDP. That means this bridge needs a Windows shared-memory reader with version checks before it can normalize and forward anything.

Assetto Corsa Rally is listed by MOZA as telemetry-supported, and public overlay tooling has started adding ACR telemetry readers. The MOZA digital-dash key table does not yet publish an ACR-specific key column, so key coverage cannot be assumed from the existing Assetto Corsa or Assetto Corsa Competizione columns.

Le Mans Ultimate has the clearest MOZA dash coverage of the three. MOZA's digital-dash telemetry table includes a `Le mans ultimate` column with 105 supported keys. Third-party dashboards may still use an rFactor-style shared-memory plugin route such as:

```text
Le Mans Ultimate/Bin64/Plugins/
```

That means a native LMU adapter should support the current shared-memory path and tolerate plugin-backed deployments. It should not listen for ordinary game UDP unless an external exporter explicitly creates one.

Detailed research notes are in [game-adapter-research.md](game-adapter-research.md).

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
- `acr-shared-memory`: planned
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

For ACE, ACR, or LMU, running `--game ace`, `--game acr`, or `--game lmu` will currently fail with a clear explanation instead of pretending a UDP bridge is enough.
