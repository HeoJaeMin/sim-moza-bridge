# ACE / ACR / LU Adapter Research

Date: 2026-05-19

This document uses:

- `ACE`: Assetto Corsa EVO
- `ACR`: Assetto Corsa Rally
- `LU` / `LMU`: Le Mans Ultimate

## Summary

ACE, ACR, and LU should not be treated like F1 25 UDP profiles. The correct shape is:

```text
game process / shared memory / plugin
  -> game-specific adapter
  -> normalized telemetry model
  -> local HUD/logging/analysis and MOZA output
```

`generic-udp` is still useful only when another exporter already emits packets that the target app understands. It is not a substitute for native ACE, ACR, or LU parsing.

## Source Findings

| Game | Current public signal | Bridge implication |
| --- | --- | --- |
| ACE | MOZA lists telemetry support. Assetto Corsa EVO 0.6 release notes mention an updated shared-memory library and official MoTeC support. Third-party dashboard docs describe basic early-access support with few exposed values. | Build a Windows shared-memory adapter with game-version/layout guards. Do not assume stable field coverage across Early Access updates. |
| ACR | MOZA lists telemetry support and SimHub lists ACR as supported. Public overlay tooling added ACR support through an accompanying `ACRallyMemReader` helper. | Add a native/helper reader adapter. Do not assume F1-style UDP or full dashboard key coverage. |
| LU / LMU | MOZA lists telemetry support and the MOZA Digital Dash key matrix has a `Le mans ultimate` column. LMU also has official native telemetry recording to DuckDB for offline analysis. Third-party dashboards may still configure a plugin-backed live path. | Implement a shared-memory/plugin-aware adapter. DuckDB recording is useful for analysis imports but is not a low-latency live HUD path. |

## MOZA Key Coverage

MOZA's game compatibility list marks telemetry support for all three:

- Assetto Corsa EVO: telemetry supported
- Assetto Corsa Rally: telemetry supported
- Le Mans Ultimate: telemetry supported

MOZA's Digital Dash Telemetry Support table is different. It is a key-by-key dashboard matrix. In the current table:

| Column | Key count in table | Notes |
| --- | ---: | --- |
| `Assetto Corsa Competizione` | 105 | Useful as an AC-family reference, but not proof of ACR coverage |
| `Assetto Corsa` | 98 | Useful as an AC-family reference, but not proof of ACE/ACR coverage |
| `Le mans ultimate` | 105 | Direct LU/LMU dashboard key coverage |
| `Assetto Corsa EVO` | none | No dedicated digital-dash column yet |
| `Assetto Corsa Rally` | none | No dedicated digital-dash column yet |

For LU, the supported key list includes the important dashboard groups: speed/RPM/gear/input, lap/gap/timing, fuel, tyre/brake temperatures, tyre wear, tyre pressures, pit/flag state, track/session metadata, car coordinates, RPM percent, wheel spin, track-position percent, location, player index, and opponent count.

For ACE and ACR, MOZA's general telemetry support means Pit House can receive some telemetry, but the public digital-dash table does not yet say exactly which `v1/gameData/...` keys are populated for these two titles. The bridge should therefore keep unsupported or uncertain values in the local HUD/logging layer until a real Pit House capture confirms key behavior.

The confirmed subset that can be mapped today is maintained in [confirmed-telemetry-mappings.md](confirmed-telemetry-mappings.md).

## Adapter Notes

### ACE

Known direction:

- Not a simple UDP stream.
- Official update notes mention shared-memory and MoTeC output.
- Early Access state means telemetry layout and field coverage can change.
- Third-party docs report only basic data values for some dashboard integrations.

Implementation target:

```text
ACE shared memory
  -> ace adapter
  -> normalized telemetry
  -> HUD/logging/MOZA output
```

Non-authoritative examples have used shared-memory names such as:

```text
Local\acevo_pmf_physics
Local\acevo_pmf_graphics
Local\acevo_pmf_static
```

These names must be verified against the installed game version before hard-coding. A probe should check available mappings and struct sizes before parsing.

### ACR

Known direction:

- MOZA marks ACR telemetry as supported.
- SimHub lists ACR as supported.
- Public overlay tooling uses an `ACRallyMemReader` helper, which points toward a local memory/helper-reader path.
- Public reports suggest early support may expose only a subset of values in some dashboard stacks.

Implementation target:

```text
ACR process/helper memory reader
  -> acr adapter
  -> normalized telemetry
  -> HUD/logging/MOZA output
```

The first ACR adapter should prioritize:

- speed
- RPM
- gear
- throttle/brake/clutch/steering
- tyre temperatures or traction-related values if exposed
- lap/stage timing where available
- stage position/distance if available

### LU / LMU

Known direction:

- MOZA marks LU telemetry as supported.
- MOZA's digital-dash key matrix directly includes `Le mans ultimate`.
- Official LMU telemetry recording exports DuckDB files for offline analysis.
- Third-party live dashboards may use shared-memory/plugin-backed integration.

Implementation target:

```text
LMU shared memory or plugin-backed live data
  -> lmu adapter
  -> normalized telemetry
  -> HUD/logging/MOZA output
```

The first LU adapter should prioritize:

- speed/RPM/gear/input
- lap timing and gap
- tyre wear, tyre temperatures, tyre pressures
- brake temperatures and brake bias
- fuel and fuel capacity
- pit limiter/pitlane/flag state
- track position percent and car coordinates

## Open Verification Tasks

Before writing native adapters, verify on a Windows machine with the games installed:

1. Whether the game exposes named Windows file mappings, plugin files, or helper processes.
2. Whether field layouts include explicit version/size markers.
3. Whether Pit House exposes ACE/ACR values under existing `v1/gameData/...` keys.
4. Whether LU key values in Pit House match the MOZA Digital Dash table.
5. Whether rev lights and wheel LEDs are driven by RPM percent, rev-light flags, or hardware-specific MOZA integration.

## Sources

- MOZA Game Compatibility List: https://support.mozaracing.com/en/support/solutions/articles/70000629729-game-support-list
- MOZA Digital Dash Telemetry Support: https://support.mozaracing.com/en/support/solutions/articles/70000627978-digital-dash-telemetry-support
- Assetto Corsa EVO 0.6 release notes: https://assettocorsa.gg/assetto-corsa-evo-early-access-06-now-available/
- SIM Dashboard Assetto Corsa EVO notes: https://www.stryder-it.de/simdashboard/help/en/For_PC_Gamers/Game_Configuration/Assetto_Corsa_EVO
- Racing Overlay ACR telemetry support notes: https://luizzak.itch.io/racing-overlay/devlog/1321475/assetto-corsa-rally-telemetry-support
- SimHub supported games: https://www.simhubdash.com/supported-games/
- Le Mans Ultimate Telemetry Recording: https://guide.lemansultimate.com/hc/en-gb/articles/14524956311695-Telemetry-Recording
- SIM Dashboard Le Mans Ultimate notes: https://www.stryder-it.de/simdashboard/help/en/For_PC_Gamers/Game_Configuration/Le_Mans_Ultimate
