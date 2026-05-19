# F1 25 / MOZA Telemetry Matrix

이 문서는 F1 25 UDP 텔레메트리 필드와 MOZA Pit House Digital Dash telemetry key를 비교한 작업 기준표입니다.

## 기준

- 확인일: 2026-05-19
- F1 25 기준: F1 25 UDP format `2025`
- MOZA 기준: MOZA Racing Support Center의 `Digital Dash Telemetry Support` 표
- 주의: MOZA 공식 표에는 `F1 25` 컬럼이 없고 `F1 24`까지만 있습니다. 그래서 F1 계열 Pit House key 지원 여부는 `F1 24` 컬럼을 기준으로 비교했습니다.

참고 원문:

- [MOZA Digital Dash Telemetry Support](https://support.mozaracing.com/en/support/solutions/articles/70000627978-digital-dash-telemetry-support)
- [EA F1 25 UDP specification forum post](https://forums.ea.com/blog/f1-games-game-info-hub-en/f1%C2%AE-25-udp-specification/12187347)
- [F1 25 UDP structure mirror used for field extraction](https://github.com/MacManley/f1-25-udp)

## 결론

MOZA 표에는 전체 telemetry name이 144개 있고, 그중 `F1 24` 컬럼에 지원 표시가 있는 key는 109개입니다. F1 25 UDP에는 그보다 훨씬 많은 raw field가 있으며, 여러 값은 단위 변환, enum label 변환, wheel array 순서 보정, player car 선택, 또는 여러 패킷 조합이 필요합니다.

현재 브리지가 MOZA로 전달되는 패킷을 실제로 수정하는 값은 `PacketCarDamageData.m_tyresWear[4]`뿐입니다. 나머지는 local HUD, logging, analysis에서 읽거나 파생할 수 있지만 Pit House에 새 `v1/gameData/...` key를 등록하지는 못합니다.

## MOZA F1 계열 key와 F1 25 source

표의 `MOZA key`는 `Telemetry.get("v1/gameData/<MOZA key>").value`의 마지막 segment입니다.

| 영역 | MOZA key | F1 25 UDP source | 비교 결과 |
| --- | --- | --- | --- |
| Race/lap | `MaxRpm` | `PacketCarStatusData.m_maxRPM` | 직접 대응 |
| Race/lap | `LapCount` | `PacketSessionData.m_totalLaps` | 직접 대응 |
| Race/lap | `CarCount`, `OpponentCount` | `PacketParticipantsData.m_numActiveCars` | `OpponentCount`는 보통 `numActiveCars - 1` 파생 |
| Race/lap | `Lap`, `CompletedLaps` | `PacketLapData.m_currentLapNum` | `CompletedLaps`는 현재 lap에서 파생 필요 |
| Race/lap | `Pos` | `PacketLapData.m_carPosition` | 직접 대응 |
| Race/lap | `LastLapTime`, `CurrentLapTime` | `PacketLapData.m_lastLapTimeInMS`, `m_currentLapTimeInMS` | ms 값을 표시 형식으로 변환 |
| Race/lap | `BestLapTime` | `PacketSessionHistoryData` 또는 `PacketFinalClassificationData.m_bestLapTimeInMS` | 직접/파생 대응 |
| Race/lap | `PlayerIndex` | `PacketHeader.m_playerCarIndex` | 직접 대응 |
| Speed/input | `SpeedKmh`, `SpeedMph`, `SpeedMs` | `PacketCarTelemetryData.m_speed` | F1 원본은 km/h, mph와 m/s는 변환 |
| Speed/input | `Rpm` | `PacketCarTelemetryData.m_engineRPM` | 직접 대응 |
| Speed/input | `Gear` | `PacketCarTelemetryData.m_gear` | 직접 대응, `0=N`, `-1=R` 표시 변환 필요 |
| Speed/input | `Throttle`, `Brake` | `PacketCarTelemetryData.m_throttle`, `m_brake` | F1 원본은 `0.0..1.0`, MOZA 표시 방식에 따라 percent 변환 가능 |
| Speed/input | `Clutch` | `PacketCarTelemetryData.m_clutch` | 직접 대응 |
| Speed/input | `Drs` | `PacketCarTelemetryData.m_drs` | 직접 대응 |
| Speed/input | `CarSettings_CurrentDisplayedRPMPercent` | `PacketCarTelemetryData.m_revLightsPercent` 또는 `m_engineRPM / m_maxRPM` | Pit House가 어느 쪽을 쓰는지 확인 필요 |
| Assist/status | `ABSLevel` | `PacketCarStatusData.m_antiLockBrakes` | F1은 `0/1`; level 표현은 label 변환 |
| Assist/status | `TCLevel` | `PacketCarStatusData.m_tractionControl` | F1은 `0/1/2`; level 표현 가능 |
| Assist/status | `BrakeBias` | `PacketCarStatusData.m_frontBrakeBias` | 직접 대응 |
| Assist/status | `DRSAllowed` | `PacketCarStatusData.m_drsAllowed` | 직접 대응 |
| Assist/status | `DRSAvailable` | `PacketCarStatusData.m_drsActivationDistance` | `> 0`이면 available로 파생 |
| Assist/status | `PitLimiter` | `PacketCarStatusData.m_pitLimiterStatus` | 직접 대응 |
| Assist/status | `IsInPit`, `Pitlane` | `PacketLapData.m_pitStatus` | `1=pitting`, `2=in pit area` enum 변환 |
| Assist/status | `EngineIgnition`, `EngineStarted` | 직접 필드 없음 | 보통 `m_engineRPM > 0` 또는 session state에서 파생 |
| Assist/status | `CarSettings_MaxGears` | `PacketCarStatusData.m_maxGears` | 직접 대응 |
| Fuel/energy | `FuelRemain`, `Fuel` | `PacketCarStatusData.m_fuelInTank` | F1 원본은 fuel mass |
| Fuel/energy | `FuelRemainLaps`, `FuelSurplusLaps` | `PacketCarStatusData.m_fuelRemainingLaps` | 직접/파생 대응 |
| Fuel/energy | `FuelClass` | `m_fuelRemainingLaps` 또는 fuel delta | label/color 파생 |
| Fuel/energy | `FuelCapacity` | `PacketCarStatusData.m_fuelCapacity` | 직접 대응 |
| Fuel/energy | `EnergyRemain`, `Ers`, `ERSPercent`, `ERSStored` | `PacketCarStatusData.m_ersStoreEnergy` | F1 원본은 Joules, percent는 보통 `4,000,000J` 기준 파생 |
| Fuel/energy | `ERSMax` | F1 고정 기준값 또는 game rule | 보통 `4,000,000J` 상수 |
| Fuel/energy | `EnergyDeployed` | `PacketCarStatusData.m_ersDeployedThisLap` | 직접 대응 |
| Fuel/energy | `EnergyHarvested` | `m_ersHarvestedThisLapMGUK + m_ersHarvestedThisLapMGUH` | 합산 파생 |
| Tyres/brakes | `TyreTempFL`, `TyreTempFR`, `TyreTempRL`, `TyreTempRR` | `PacketCarTelemetryData.m_tyresSurfaceTemperature[4]` | F1 wheel order `RL, RR, FL, FR`를 key 이름으로 매핑 필요 |
| Tyres/brakes | `TyreTempFLI`, `TyreTempFRI`, `TyreTempRLI`, `TyreTempRRI` | `PacketCarTelemetryData.m_tyresInnerTemperature[4]` | wheel order 매핑 필요 |
| Tyres/brakes | `TyreTempFL&F`, `TyreTempFLI&F`, `TyreTempFR&F`, `TyreTempFRI&F`, `TyreTempRL&F`, `TyreTempRLI&F`, `TyreTempRR&F`, `TyreTempRRI&F` | surface/inner tyre temperature | Fahrenheit 표시 key로 보면 단위 변환 |
| Tyres/brakes | `TyrePressureFL`, `TyrePressureFR`, `TyrePressureRL`, `TyrePressureRR` | `PacketCarTelemetryData.m_tyresPressure[4]` | F1 원본은 PSI, wheel order 매핑 필요 |
| Tyres/brakes | `BrakeTempFL`, `BrakeTempFR`, `BrakeTempRL`, `BrakeTempRR` | `PacketCarTelemetryData.m_brakesTemperature[4]` | Celsius, wheel order 매핑 필요 |
| Tyres/brakes | `BrakeTempFL&F`, `BrakeTempFR&F`, `BrakeTempRL&F`, `BrakeTempRR&F` | brake temperature | Fahrenheit 표시 key로 보면 단위 변환 |
| Tyres/brakes | `TrackTemp`, `AirTemp` | `PacketSessionData.m_trackTemperature`, `m_airTemperature` | 직접 대응 |
| Tyres/brakes | `TrackTemp&F`, `AirTemp&F` | track/air temperature | Fahrenheit 표시 key로 보면 단위 변환 |
| Damage/wear | `TyreWearFL`, `TyreWearFR`, `TyreWearRL`, `TyreWearRR` | `PacketCarDamageData.m_tyresWear[4]` | wheel order 보정 필요. 현재 packet-level remap 구현됨 |
| Damage/wear | `WingWearFL`, `WingWearFR` | `PacketCarDamageData.m_frontLeftWingDamage`, `m_frontRightWingDamage` | 직접 대응 |
| Damage/wear | `EngineWear` | `PacketCarDamageData.m_engineDamage` 또는 component wear fields | 단일 값 선택 정책 필요 |
| Damage/wear | `GearBoxWear` | `PacketCarDamageData.m_gearBoxDamage` | 직접 대응 |
| Flags/session | `YellowFlag`, `GreenFlag` | `PacketCarStatusData.m_vehicleFiaFlags`, `PacketSessionData.m_marshalZones[]` | enum/zone 해석 필요 |
| Flags/session | `SectorIndex` | `PacketLapData.m_sector` | 직접 대응 |
| Flags/session | `Sector1Time`, `Sector2Time` | `PacketLapData.m_sector1Time*`, `m_sector2Time*` | minute/ms 조합 변환 |
| Flags/session | `SessionTimeLeft` | `PacketSessionData.m_sessionTimeLeft` | 직접 대응 |
| Flags/session | `LapInvalidated` | `PacketLapData.m_currentLapInvalid` | 직접 대응 |
| Flags/session | `SessionTypeName` | `PacketSessionData.m_sessionType` | enum label 변환 |
| Flags/session | `TrackId` | `PacketSessionData.m_trackId` | 직접 대응 |
| Flags/session | `MapName` | `PacketSessionData.m_trackId` | track id to name label 변환 |
| Flags/session | `TrackLength` | `PacketSessionData.m_trackLength` | 직접 대응 |
| Motion/map | `Heading` | `PacketMotionData.m_yaw` | radians to display heading 변환 |
| Motion/map | `Pitch`, `Roll` | `PacketMotionData.m_pitch`, `m_roll` 또는 `PacketMotionExData.m_chassisPitch` | 직접/파생 대응 |
| Motion/map | `CarCoordinates01`, `CarCoordinates02`, `CarCoordinates03`, `Location` | `PacketMotionData.m_worldPositionX/Y/Z` | 이름별 축 매핑 필요 |
| Motion/map | `TrackPositionPercent` | `PacketLapData.m_lapDistance / PacketSessionData.m_trackLength` | 파생 |
| Motion/map | `WheelSpin` | `PacketMotionExData.m_wheelSlipRatio[4]`, `m_wheelSpeed[4]` | 파생 |
| Identity | `CarId`, `CarModel` | `PacketParticipantsData.m_teamId`, `m_techLevel`, `m_carNumber` | Pit House 표시 정책 필요 |
| Identity | `PlayerName` | `PacketParticipantsData.m_name` | online name 설정에 영향 |
| Identity | `Gamename` | game/profile constant | F1 packet field라기보다 adapter label |

## MOZA global key지만 F1 24 컬럼에 표시가 없는 key

아래 key는 MOZA 전체 표에는 있지만 `F1 24` 컬럼에는 지원 표시가 없습니다. F1 25 UDP에 비슷한 source가 있더라도 Pit House F1 대시에서 그대로 쓸 수 있다고 보면 안 됩니다.

| MOZA key | F1 25에 비슷한 source가 있는가 | 비고 |
| --- | --- | --- |
| `Gap`, `EstimatedLapTime` | 있음 | `m_deltaToCarInFront*`, `m_deltaToRaceLeader*`, lap pace로 파생 가능하지만 F1 컬럼 미지원 |
| `ABS`, `TC` | 있음 | `ABSLevel`, `TCLevel`은 지원 표시가 있으나 단순 boolean key는 F1 컬럼 미지원 |
| `ECUMap`, `Boost` | 제한적/없음 | F1 25 현대 F1 ERS/engine model과 직접 1:1 아님 |
| `TyreTempFLM`, `TyreTempFRM`, `TyreTempRLM`, `TyreTempRRM` | 없음 | F1은 surface/inner만 제공 |
| `TyreTempFLO&F`, `TyreTempFRO&F`, `TyreTempRLO&F`, `TyreTempRRO&F`, `TyreTempFLO`, `TyreTempFRO`, `TyreTempRL0`, `TyreTempRRO` | 없음 | outer/middle 계열 key로 보이며 F1 25 UDP에는 직접 값 없음. `TyreTempRL0`는 MOZA 원문 표기 유지 |
| `FuelTemp`, `WaterTemperature`, `OilPressure` | 없음 | F1 25 UDP에는 직접 대응 field 없음 |
| `WingWearR` | 있음 | `m_rearWingDamage`가 있지만 MOZA F1 컬럼에는 지원 표시 없음 |
| `ReverseLight` | 파생 가능 | `m_gear == -1`로 파생 가능하지만 F1 컬럼 미지원 |
| `BlueFlag`, `WhiteFlag`, `RedFlag`, `Flag_Black` | 부분적으로 있음 | FIA flag, marshal zone, event/result status에서 추론 가능하지만 key 지원 표시 없음 |
| `AccX`, `AccY`, `AccZ`, `GlobalAccelerationG` | 부분적으로 있음 | F1 Motion packet에 G-force와 velocity가 있으나 key 지원 표시 없음 |
| `SectorsCount` | 파생 가능 | F1은 일반적으로 3 sector. key 지원 표시 없음 |
| `Spectating` | 있음 | `m_isSpectating`이 있지만 key 지원 표시 없음 |
| `ReplayMode`, `Ontrack` | 명확한 직접 field 없음 | 다른 게임 adapter용 key로 보는 것이 안전 |

## F1 25에는 있지만 MOZA key로 보존하기 어려운 데이터

F1 25 UDP의 raw field는 MOZA key보다 더 넓습니다. 다음 정보는 F1 packet에는 있지만 Pit House Digital Dash key와 직접 대응하지 않습니다.

| F1 packet | MOZA key 대응이 약한 값 |
| --- | --- |
| `PacketCarSetupData` | front/rear wing, differential, camber, toe, suspension, anti-roll bar, ride height, brake pressure, engine braking, setup tyre pressure, ballast, fuel load |
| `PacketSessionData` | weather forecast samples, marshal zone list, assist settings, rule set, safety car/red flag counts, weekend structure, race settings |
| `PacketParticipantsData`, `PacketLobbyInfoData` | AI controlled, driver/team/nationality/platform metadata, telemetry privacy flags, livery colors, lobby ready status |
| `PacketEventData` | fastest lap event, retirement reason, penalty details, speed trap event, flashback, button flags, collision/overtake/safety-car events |
| `PacketFinalClassificationData` | points, penalties, tyre stint history, result reason, total race time details |
| `PacketSessionHistoryData` | every lap/sector history and stint history beyond the few dashboard lap-time keys |
| `PacketTyreSetsData` | available tyre sets, recommended session, lifespan, usable life, fitted index |
| `PacketMotionExData` | suspension position/velocity/acceleration, wheel slip angle, wheel forces, aero heights, roll angles, chassis yaw/pitch, camber, camber gain |
| `PacketLapPositionsData` | full lap-by-lap position chart data |
| `PacketTimeTrialData` | personal best/rival datasets and assists for time trial comparison |

## Bridge implementation priority

1. Keep `m_tyresWear[4]` packet remap as an opt-in fix only.
2. Add parser coverage for values that already have MOZA F1-family keys before inventing local-only HUD fields.
3. Treat gap/delta, setup, forecast, event, and tyre-set data as local HUD/report features unless Pit House exposes a matching key.
4. For any wheel-array key, always normalize from F1 order `RL, RR, FL, FR` to named corners before display or remap.
