# Driving Analysis

The bridge can produce local F1 25 driving analysis without changing the packet stream sent to MOZA Pit House.

## Inputs

The analyzer uses the player car from these F1 25 UDP packets:

| Packet | Use |
| --- | --- |
| `PacketCarTelemetryData` | throttle, brake, steer, speed, gear, RPM, DRS, REV, tyre temps |
| `PacketLapData` | lap number, lap distance, current lap time, invalid lap flag, pit status, front/leader deltas |
| `PacketSessionData` | track length for segment sizing |
| `PacketCarStatusData` | fuel, brake bias, ERS, tyre compound and tyre age |
| `PacketCarDamageData` | tyre wear and damage |

## Outputs

```bash
cargo run -- --corner-log corners.csv --analysis-report analysis.md
```

`corners.csv` appends one row per segment for each completed lap:

- clean lap flag
- segment distance range
- sample count
- min/avg/max speed
- max brake
- max throttle
- average and max absolute steering
- coarse phase label: `entry`, `mid`, `exit`, or `straight`

`analysis.md` is overwritten on every completed lap and is designed to be opened while testing setup changes.

## Clean Lap Detection

A completed lap is marked clean when:

- F1 25 did not flag the lap as invalid
- pit status was not active during the lap
- enough input samples were captured

Invalid laps are still reported, but setup recommendations should be ignored for them.

## Setup Candidates

The setup section is heuristic. It looks for repeatable signals:

| Signal | Candidate |
| --- | --- |
| high mid-corner steering demand with low throttle | more front grip: front wing +1, softer front ARB, or slightly lower front tyre pressure |
| high exit throttle with steering correction | rear traction: on-throttle diff down, rear wing +1, or slightly lower rear tyre pressure |
| high brake and steering overlap on entry | review brake bias and off-throttle differential |
| front tyre wear/temp higher than rear | front-limited balance candidate |
| rear tyre wear/temp higher than front | rear-limited traction or stability candidate |

Use the report as an A/B checklist. Change one setup item at a time, then compare clean laps with similar fuel load, tyre age, ERS mode, and traffic.
