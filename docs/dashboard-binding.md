# MOZA Dashboard Binding Notes

MOZA Dash Studio can bind text, gauge, color, and animation expressions to telemetry values through:

```js
Telemetry.get("v1/gameData/Rpm").value
```

The prefix is important:

- `v1` is the binding API namespace.
- `gameData` is the telemetry namespace Pit House fills from supported games.
- The final segment is the MOZA telemetry name shown in the Digital Dash support list.

## F1 25 Dashboard Implication

F1 25 sends raw binary UDP packets. Pit House parses those packets and publishes selected values into `v1/gameData/...` keys. A UDP bridge can alter packet values before Pit House receives them, but it cannot add arbitrary dashboard keys.

That means:

- Changing `m_tyresWear[4]` can affect `v1/gameData/TyreWearFL` and related tyre wear keys if Pit House maps those values from the Car Damage packet.
- Creating a new `v1/gameData/BehindGap` key is not expected to work through UDP packet forwarding alone.
- Behind gap would need to be packed into an existing Pit House-exposed field, or rendered in a separate dashboard that reads from this bridge directly.

## Example Expressions

DRS badge:

```js
Telemetry.get("v1/gameData/DRSAvailable").value ? "DRS" : ""
```

ERS color:

```js
Telemetry.get("v1/gameData/ERSPercent").value < 20 ? "#FF3B30" : "#21D17C"
```

RPM warning:

```js
Telemetry.get("v1/gameData/CarSettings_CurrentDisplayedRPMPercent").value > 95 ? "#FF3B30" : "#E8EDF2"
```

Fuel laps with one decimal:

```js
Telemetry.get("v1/gameData/FuelRemainLaps").value.toFixed(1)
```
