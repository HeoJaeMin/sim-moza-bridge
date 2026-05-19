# Confirmed Telemetry Mappings

Date: 2026-05-19

This document maps only values that are defensible from the sources already collected.

Certainty terms:

- `source+key`: the bridge currently parses the source value and a matching MOZA `v1/gameData/...` key exists for the relevant game family.
- `derived`: the source value is known, but the dashboard value needs a mechanical unit conversion or boolean derivation.
- `local-only`: the source value is known, but the currently confirmed MOZA key support is missing or unsafe to assume.
- `output-only`: MOZA key support is confirmed, but the native game adapter has not been implemented yet.

The bridge still cannot register new Pit House keys. These mappings define the normalized adapter target and the values that can feed the local HUD/logging layer.

## F1 25 Source To MOZA Keys

These mappings are based on the current F1 25 parser coverage in `src/f1/*` and the MOZA Digital Dash table's F1-family keys.

### Speed, Input, REV, And DRS

| MOZA key | F1 25 source | Certainty | Conversion |
| --- | --- | --- | --- |
| `SpeedKmh` | `PacketCarTelemetryData.m_speed` | `source+key` | none |
| `SpeedMph` | `PacketCarTelemetryData.m_speed` | `derived` | `kmh * 0.621371` |
| `SpeedMs` | `PacketCarTelemetryData.m_speed` | `derived` | `kmh / 3.6` |
| `Rpm` | `PacketCarTelemetryData.m_engineRPM` | `source+key` | none |
| `Gear` | `PacketCarTelemetryData.m_gear` | `source+key` | `-1=R`, `0=N`, positive numbers unchanged |
| `Throttle` | `PacketCarTelemetryData.m_throttle` | `source+key` | normalized `0.0..1.0`; UI may display percent |
| `Brake` | `PacketCarTelemetryData.m_brake` | `source+key` | normalized `0.0..1.0`; UI may display percent |
| `Clutch` | `PacketCarTelemetryData.m_clutch` | `source+key` | F1 raw clutch value |
| `Drs` | `PacketCarTelemetryData.m_drs` | `source+key` | `0/1` or boolean |
| `CarSettings_CurrentDisplayedRPMPercent` | `PacketCarTelemetryData.m_revLightsPercent` | `derived` | F1 already provides percent; wheel LED behavior still needs hardware validation |

### Tyres, Brakes, And Pressures

F1 wheel arrays are always `0=RL`, `1=RR`, `2=FL`, `3=FR`. Every named-corner key below must be populated after that reorder.

| MOZA key | F1 25 source | Certainty | Conversion |
| --- | --- | --- | --- |
| `TyreTempFL` | `m_tyresSurfaceTemperature[2]` | `source+key` | Celsius |
| `TyreTempFR` | `m_tyresSurfaceTemperature[3]` | `source+key` | Celsius |
| `TyreTempRL` | `m_tyresSurfaceTemperature[0]` | `source+key` | Celsius |
| `TyreTempRR` | `m_tyresSurfaceTemperature[1]` | `source+key` | Celsius |
| `TyreTempFLI` | `m_tyresInnerTemperature[2]` | `source+key` | Celsius |
| `TyreTempFRI` | `m_tyresInnerTemperature[3]` | `source+key` | Celsius |
| `TyreTempRLI` | `m_tyresInnerTemperature[0]` | `source+key` | Celsius |
| `TyreTempRRI` | `m_tyresInnerTemperature[1]` | `source+key` | Celsius |
| `TyreTempFL&F`, `TyreTempFR&F`, `TyreTempRL&F`, `TyreTempRR&F` | surface tyre temps above | `derived` | `C * 9 / 5 + 32` |
| `TyreTempFLI&F`, `TyreTempFRI&F`, `TyreTempRLI&F`, `TyreTempRRI&F` | inner tyre temps above | `derived` | `C * 9 / 5 + 32` |
| `BrakeTempFL` | `m_brakesTemperature[2]` | `source+key` | Celsius |
| `BrakeTempFR` | `m_brakesTemperature[3]` | `source+key` | Celsius |
| `BrakeTempRL` | `m_brakesTemperature[0]` | `source+key` | Celsius |
| `BrakeTempRR` | `m_brakesTemperature[1]` | `source+key` | Celsius |
| `BrakeTempFL&F`, `BrakeTempFR&F`, `BrakeTempRL&F`, `BrakeTempRR&F` | brake temps above | `derived` | `C * 9 / 5 + 32` |
| `TyrePressureFL` | `m_tyresPressure[2]` | `source+key` | PSI |
| `TyrePressureFR` | `m_tyresPressure[3]` | `source+key` | PSI |
| `TyrePressureRL` | `m_tyresPressure[0]` | `source+key` | PSI |
| `TyrePressureRR` | `m_tyresPressure[1]` | `source+key` | PSI |
| `TyreWearFL` | `PacketCarDamageData.m_tyresWear[2]` | `source+key` | percent |
| `TyreWearFR` | `PacketCarDamageData.m_tyresWear[3]` | `source+key` | percent |
| `TyreWearRL` | `PacketCarDamageData.m_tyresWear[0]` | `source+key` | percent |
| `TyreWearRR` | `PacketCarDamageData.m_tyresWear[1]` | `source+key` | percent |

### Lap, Session, And Position

| MOZA key | F1 25 source | Certainty | Conversion |
| --- | --- | --- | --- |
| `LapCount` | `PacketSessionData.m_totalLaps` | `source+key` | none |
| `TrackLength` | `PacketSessionData.m_trackLength` | `source+key` | meters |
| `TrackId` | `PacketSessionData.m_trackId` | `source+key` | numeric id |
| `SessionTimeLeft` | `PacketSessionData.m_sessionTimeLeft` | `source+key` | seconds |
| `TrackTemp` | `PacketSessionData.m_trackTemperature` | `source+key` | Celsius |
| `AirTemp` | `PacketSessionData.m_airTemperature` | `source+key` | Celsius |
| `TrackTemp&F`, `AirTemp&F` | track/air temps above | `derived` | `C * 9 / 5 + 32` |
| `Lap` | `PacketLapData.m_currentLapNum` | `source+key` | none |
| `Pos` | `PacketLapData.m_carPosition` | `source+key` | none |
| `LastLapTime` | `PacketLapData.m_lastLapTimeInMS` | `source+key` | milliseconds; UI formats as time |
| `CurrentLapTime` | `PacketLapData.m_currentLapTimeInMS` | `source+key` | milliseconds; UI formats as time |
| `SectorIndex` | `PacketLapData.m_sector` | `source+key` | `0=S1`, `1=S2`, `2=S3` |
| `Sector1Time` | `PacketLapData.m_sector1Time*` | `source+key` | minute/ms parts combined to milliseconds |
| `Sector2Time` | `PacketLapData.m_sector2Time*` | `source+key` | minute/ms parts combined to milliseconds |
| `LapInvalidated` | `PacketLapData.m_currentLapInvalid` | `source+key` | boolean |
| `IsInPit` | `PacketLapData.m_pitStatus` | `derived` | true when pit status is not `0` |
| `Pitlane` | `PacketLapData.m_pitStatus` | `derived` | true when pitting or in pit area |
| `PlayerIndex` | `PacketHeader.m_playerCarIndex` | `source+key` | none |
| `TrackPositionPercent` | `m_lapDistance / m_trackLength` | `derived` | requires `PacketLapData` plus `PacketSessionData` |

### Status, Fuel, ERS, And Assists

| MOZA key | F1 25 source | Certainty | Conversion |
| --- | --- | --- | --- |
| `MaxRpm` | `PacketCarStatusData.m_maxRPM` | `source+key` | none |
| `ABSLevel` | `PacketCarStatusData.m_antiLockBrakes` | `source+key` | F1 `0/1` |
| `TCLevel` | `PacketCarStatusData.m_tractionControl` | `source+key` | F1 `0/1/2` |
| `BrakeBias` | `PacketCarStatusData.m_frontBrakeBias` | `source+key` | percent |
| `DRSAllowed` | `PacketCarStatusData.m_drsAllowed` | `source+key` | boolean |
| `DRSAvailable` | `PacketCarStatusData.m_drsActivationDistance` | `derived` | true when distance is greater than `0` |
| `PitLimiter` | `PacketCarStatusData.m_pitLimiterStatus` | `source+key` | boolean |
| `CarSettings_MaxGears` | `PacketCarStatusData.m_maxGears` | `source+key` | none |
| `FuelRemain` | `PacketCarStatusData.m_fuelInTank` | `source+key` | F1 fuel mass |
| `Fuel` | `PacketCarStatusData.m_fuelInTank` | `source+key` | same source as `FuelRemain` |
| `FuelCapacity` | `PacketCarStatusData.m_fuelCapacity` | `source+key` | F1 fuel mass capacity |
| `FuelRemainLaps` | `PacketCarStatusData.m_fuelRemainingLaps` | `source+key` | laps |
| `EnergyRemain`, `Ers`, `ERSStored` | `PacketCarStatusData.m_ersStoreEnergy` | `source+key` | Joules |
| `ERSPercent` | `PacketCarStatusData.m_ersStoreEnergy` | `derived` | `ersStoreEnergy / 4_000_000 * 100` |
| `ERSMax` | F1 rule constant | `derived` | `4_000_000J` |
| `EnergyDeployed` | `PacketCarStatusData.m_ersDeployedThisLap` | `source+key` | Joules |

### Damage

| MOZA key | F1 25 source | Certainty | Conversion |
| --- | --- | --- | --- |
| `WingWearFL` | `PacketCarDamageData.m_frontLeftWingDamage` | `source+key` | percent |
| `WingWearFR` | `PacketCarDamageData.m_frontRightWingDamage` | `source+key` | percent |
| `GearBoxWear` | `PacketCarDamageData.m_gearBoxDamage` | `source+key` | percent |
| `EngineWear` | `PacketCarDamageData.m_engineDamage` | `source+key` | percent |

## F1 25 Confirmed Local-Only Fields

These are safe to map inside the bridge's normalized model, HUD, logging, or analysis, but they should not be sent as MOZA Pit House keys unless a matching supported key is confirmed.

| Local field | F1 25 source | Why local-only |
| --- | --- | --- |
| `frontGapMs` | `m_deltaToCarInFrontMinutesPart`, `m_deltaToCarInFrontMSPart` | MOZA `Gap` is not marked as F1-family supported in the collected table |
| `leaderGapMs` | `m_deltaToRaceLeaderMinutesPart`, `m_deltaToRaceLeaderMSPart` | no confirmed `LeaderGap`/`BehindGap` Pit House key |
| `steer` | `PacketCarTelemetryData.m_steer` | no confirmed MOZA F1 dashboard key |
| `revLightsBitValue` | `PacketCarTelemetryData.m_revLightsBitValue` | useful for local/wheel LED work, but no confirmed MOZA key |
| `tyreDamageFL/FR/RL/RR` | `PacketCarDamageData.m_tyresDamage[4]` | no confirmed MOZA key |
| `tyreBlistersFL/FR/RL/RR` | `PacketCarDamageData.m_tyreBlisters[4]` | no confirmed MOZA key |
| `rearWingDamage` | `PacketCarDamageData.m_rearWingDamage` | `WingWearR` exists globally but is not confirmed for F1-family support |

## F1 25 Raw-Known But Policy-Pending Keys

These have plausible F1 sources and MOZA key names, but the value semantics need a project policy before treating them as live mapped values.

| MOZA key | F1 25 source | Work needed |
| --- | --- | --- |
| `FuelSurplusLaps` | `PacketCarStatusData.m_fuelRemainingLaps` plus race target policy | define whether this means remaining laps or surplus delta before mapping |

## LU / LMU Output Mapping Target

The raw LU/LMU adapter is not implemented yet, so source fields are not mapped here. The following MOZA keys are confirmed as supported by the `Le mans ultimate` column and should be the first output target once the live adapter exists.

| Group | Confirmed LU/LMU MOZA keys |
| --- | --- |
| Lap/race | `MaxRpm`, `LapCount`, `CarCount`, `Lap`, `Pos`, `Gap`, `EstimatedLapTime`, `LastLapTime`, `BestLapTime`, `CurrentLapTime`, `CompletedLaps`, `LapInvalidated`, `SessionTypeName`, `PlayerIndex`, `OpponentCount` |
| Speed/input | `SpeedMph`, `SpeedKmh`, `SpeedMs`, `Rpm`, `Gear`, `Throttle`, `Brake`, `Clutch`, `BrakeBias`, `Boost`, `CarSettings_MaxGears`, `CarSettings_CurrentDisplayedRPMPercent`, `WheelSpin` |
| Fuel | `FuelRemain`, `FuelCapacity`, `FuelTemp`, `Fuel` |
| Tyres | `TyreWearFL`, `TyreWearFR`, `TyreWearRL`, `TyreWearRR`, `TyrePressureFL`, `TyrePressureFR`, `TyrePressureRL`, `TyrePressureRR`, `TyreTempFL`, `TyreTempFR`, `TyreTempRL`, `TyreTempRR`, `TyreTempFLI`, `TyreTempFRI`, `TyreTempRLI`, `TyreTempRRI`, `TyreTempFLM`, `TyreTempFRM`, `TyreTempRLM`, `TyreTempRRM`, `TyreTempFLO`, `TyreTempFRO`, `TyreTempRL0`, `TyreTempRRO` |
| Tyres Fahrenheit | `TyreTempFL&F`, `TyreTempFLI&F`, `TyreTempFLO&F`, `TyreTempFR&F`, `TyreTempFRI&F`, `TyreTempFRO&F`, `TyreTempRL&F`, `TyreTempRLI&F`, `TyreTempRLO&F`, `TyreTempRR&F`, `TyreTempRRI&F`, `TyreTempRRO&F` |
| Brakes | `BrakeTempFL`, `BrakeTempFR`, `BrakeTempRL`, `BrakeTempRR`, `BrakeTempFL&F`, `BrakeTempFR&F`, `BrakeTempRL&F`, `BrakeTempRR&F` |
| Environment | `TrackTemp`, `AirTemp`, `TrackTemp&F`, `AirTemp&F`, `WaterTemperature` |
| Pit/flags/session | `IsInPit`, `EngineIgnition`, `Pitlane`, `PitLimiter`, `YellowFlag`, `SectorIndex`, `Sector1Time`, `Sector2Time`, `EngineStarted`, `SectorsCount` |
| Motion/map | `Pitch`, `Roll`, `MapName`, `CarCoordinates01`, `CarCoordinates02`, `CarCoordinates03`, `TrackLength`, `TrackId`, `TrackPositionPercent`, `Location` |
| Identity | `CarId`, `PlayerName`, `CarModel`, `Gamename` |

## ACE / ACR Mapping Boundary

ACE and ACR are not mapped to Pit House keys yet.

Confirmed facts:

- MOZA's game compatibility list marks telemetry support for both games.
- The Digital Dash key matrix has no dedicated `Assetto Corsa EVO` or `Assetto Corsa Rally` column.
- ACE points to shared-memory telemetry, and ACR points to a native/helper reader path.

Therefore:

- Do not claim exact ACE/ACR `v1/gameData/...` key coverage yet.
- Do map their future adapters into the same normalized core fields as F1/LU when the raw source fields are verified: speed, RPM, gear, throttle, brake, clutch, steering, tyre temps, tyre pressures, tyre wear if exposed, brake temps, lap timing, gap/timing if exposed, and track position if exposed.
- Keep uncertain ACE/ACR fields in the local HUD/logging layer until a real Pit House capture confirms the dashboard keys.

## Implementation Priority

1. Wire the existing F1 parsed values above into a normalized `gameData` map for the local HUD/API.
2. Keep F1 `Gap` as `local-only` unless Pit House confirms a supported key for F1.
3. Implement LU/LMU adapter against the output-only key groups first because MOZA key support is already confirmed.
4. Add ACE/ACR probes before hard-coding shared-memory struct names or field offsets.
