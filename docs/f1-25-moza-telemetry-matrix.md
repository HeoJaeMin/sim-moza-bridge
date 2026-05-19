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

MOZA 표에는 전체 telemetry name이 144개 있고, 그중 `F1 24` 컬럼에 지원 표시가 있는 key는 109개입니다. F1 25 UDP에는 그보다 훨씬 많은 raw field가 있으며, 여러 값은 단위 변환, enum label 변환, wheel array 순서 보정, player car 선택, 또는 여러 패킷 조합이 필요합니다. 대표적으로 차간 gap은 F1 25 UDP에 `m_deltaToCarInFront*`, `m_deltaToRaceLeader*`로 존재하지만 MOZA F1 계열 key로는 지원 표시가 없습니다.

현재 브리지가 MOZA로 전달되는 패킷을 실제로 수정하는 값은 `PacketCarDamageData.m_tyresWear[4]`뿐입니다. 나머지는 local HUD, logging, analysis에서 읽거나 파생할 수 있지만 Pit House에 새 `v1/gameData/...` key를 등록하지는 못합니다.

## F1 raw wheel array 규칙

F1 25 UDP 문서의 실제 타이어 웨어 필드명은 `typeWear`가 아니라 `m_tyresWear[4]`입니다. `PacketCarDamageData`는 22대 차량의 `CarDamageData` 배열을 들고 있으므로 플레이어 차량의 타이어 웨어는 다음 path로 읽습니다.

```text
PacketCarDamageData.m_carDamageData[PacketHeader.m_playerCarIndex].m_tyresWear[index]
```

F1 25의 모든 wheel array는 같은 index 순서를 씁니다.

| F1 array index | Corner | 약어 |
| --- | --- | --- |
| `0` | Rear Left | `RL` |
| `1` | Rear Right | `RR` |
| `2` | Front Left | `FL` |
| `3` | Front Right | `FR` |

따라서 MOZA 이름 기준 key로 표현하려면 index를 그대로 쓰면 안 되고 아래처럼 corner name으로 다시 매핑해야 합니다.

| MOZA key | F1 raw field path |
| --- | --- |
| `TyreWearFL` | `m_carDamageData[playerCarIndex].m_tyresWear[2]` |
| `TyreWearFR` | `m_carDamageData[playerCarIndex].m_tyresWear[3]` |
| `TyreWearRL` | `m_carDamageData[playerCarIndex].m_tyresWear[0]` |
| `TyreWearRR` | `m_carDamageData[playerCarIndex].m_tyresWear[1]` |

현재 `--fix-tyre-wear-order`는 Pit House가 배열을 이름 기준이 아니라 앞에서부터 `FL, FR, RL, RR`처럼 읽는 경우를 보정하기 위한 opt-in 기능입니다. Pit House가 이미 F1 spec대로 `TyreWearFL = m_tyresWear[2]`로 읽고 있다면 이 옵션을 켜면 오히려 값이 틀어집니다.

이 index 규칙은 타이어 웨어만의 문제가 아닙니다. F1 25에서 아래 wheel array들도 같은 `0=RL, 1=RR, 2=FL, 3=FR` 순서를 씁니다.

| F1 packet | F1 field | MOZA 이름 기준으로 매핑이 필요한 key |
| --- | --- | --- |
| `PacketCarDamageData` | `m_tyresWear[4]` | `TyreWearFL`, `TyreWearFR`, `TyreWearRL`, `TyreWearRR` |
| `PacketCarDamageData` | `m_tyresDamage[4]` | 직접 대응 key는 MOZA F1 계열 표에 없음. 로컬 HUD/report에서는 corner name으로 매핑 필요 |
| `PacketCarDamageData` | `m_brakesDamage[4]` | 직접 대응 key는 MOZA F1 계열 표에 없음. 로컬 HUD/report에서는 corner name으로 매핑 필요 |
| `PacketCarDamageData` | `m_tyreBlisters[4]` | 직접 대응 key는 MOZA F1 계열 표에 없음. 로컬 HUD/report에서는 corner name으로 매핑 필요 |
| `PacketCarTelemetryData` | `m_brakesTemperature[4]` | `BrakeTempFL`, `BrakeTempFR`, `BrakeTempRL`, `BrakeTempRR` |
| `PacketCarTelemetryData` | `m_tyresSurfaceTemperature[4]` | `TyreTempFL`, `TyreTempFR`, `TyreTempRL`, `TyreTempRR` |
| `PacketCarTelemetryData` | `m_tyresInnerTemperature[4]` | `TyreTempFLI`, `TyreTempFRI`, `TyreTempRLI`, `TyreTempRRI` |
| `PacketCarTelemetryData` | `m_tyresPressure[4]` | `TyrePressureFL`, `TyrePressureFR`, `TyrePressureRL`, `TyrePressureRR` |
| `PacketMotionExData` | `m_wheelSpeed[4]`, `m_wheelSlipRatio[4]`, `m_wheelSlipAngle[4]`, `m_wheelLatForce[4]`, `m_wheelLongForce[4]`, `m_wheelVertForce[4]`, `m_wheelCamber[4]`, `m_wheelCamberGain[4]` | Pit House F1 계열 key로 직접 보존하기 어렵고, 로컬 분석/HUD에서 corner name으로 매핑해야 함 |

## 주요 F1 raw 지표 상세 매핑

아래 표는 대시보드나 분석에 쓰기 쉬운 단위로 F1 25 raw field를 다시 묶은 것입니다. `m_*Data[playerCarIndex]`라고 적힌 항목은 packet header의 `m_playerCarIndex`로 플레이어 차량 row를 골라야 합니다.

### Gap, lap, race position

| 표시/분석 지표 | F1 25 raw field | MOZA F1 key 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| 앞차와의 gap | `PacketLapData.m_lapData[playerCarIndex].m_deltaToCarInFrontMinutesPart`, `m_deltaToCarInFrontMSPart` | MOZA 표의 `Gap`은 F1 24 컬럼 미지원. `FrontGap` 같은 key도 표에 없음 | `minutes * 60000 + ms`로 ms gap 생성. `minutes == 255`는 invalid로 처리. 로컬 HUD/report용 |
| 선두와의 gap | `PacketLapData.m_lapData[playerCarIndex].m_deltaToRaceLeaderMinutesPart`, `m_deltaToRaceLeaderMSPart` | 직접 key 없음 | leader gap으로 별도 표시. MOZA Pit House key 주입은 불가 |
| 세이프티카 delta | `PacketLapData.m_lapData[playerCarIndex].m_safetyCarDelta` | 직접 key 없음 | safety car/VSC HUD 또는 분석용 |
| 현재 순위 | `m_lapData[playerCarIndex].m_carPosition` | `Pos` | 직접 대응 |
| 현재 lap | `m_lapData[playerCarIndex].m_currentLapNum` | `Lap` | 직접 대응. completed laps는 현재 lap에서 파생 |
| completed laps | `m_currentLapNum`, start/finish crossing state | `CompletedLaps` | F1 raw에 그대로 있는 값이 아니라 lap state에서 파생 |
| 현재 lap time | `m_currentLapTimeInMS` | `CurrentLapTime` | ms를 표시 형식으로 변환 |
| 직전 lap time | `m_lastLapTimeInMS` | `LastLapTime` | ms를 표시 형식으로 변환 |
| best lap time | `PacketSessionHistoryData.m_lapHistoryData[].m_lapTimeInMS` 또는 `PacketFinalClassificationData.m_bestLapTimeInMS` | `BestLapTime` | session history/final classification 파서 필요 |
| sector index | `m_lapData[playerCarIndex].m_sector` | `SectorIndex` | `0=S1`, `1=S2`, `2=S3` label 변환 |
| sector 1/2 time | `m_sector1TimeMinutesPart + m_sector1TimeMSPart`, `m_sector2TimeMinutesPart + m_sector2TimeMSPart` | `Sector1Time`, `Sector2Time` | minute/ms 조합 변환 |
| lap invalid | `m_currentLapInvalid` | `LapInvalidated` | 직접 대응 |
| penalties/warnings | `m_penalties`, `m_totalWarnings`, `m_cornerCuttingWarnings` | 직접 key 없음 | 로컬 HUD/report용 |
| pit status | `m_pitStatus` | `IsInPit`, `Pitlane` | `0=none`, `1=pitting`, `2=in pit area` 변환 |
| pit stop timing | `m_pitLaneTimerActive`, `m_pitLaneTimeInLaneInMS`, `m_pitStopTimerInMS`, `m_pitStopShouldServePen` | 제한적 | Pit HUD/report용. MOZA 표 직접 key 부족 |
| driver/result status | `m_driverStatus`, `m_resultStatus` | 제한적 | enum label 변환. inactive/retired filtering에 필요 |
| track position percent | `m_lapDistance / PacketSessionData.m_trackLength` | `TrackPositionPercent` | 파생 가능 |

### Speed, input, REV, DRS

| 표시/분석 지표 | F1 25 raw field | MOZA F1 key 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| speed km/h | `PacketCarTelemetryData.m_carTelemetryData[playerCarIndex].m_speed` | `SpeedKmh` | 직접 대응 |
| speed mph / m/s | `m_speed` | `SpeedMph`, `SpeedMs` | 단위 변환 |
| throttle | `m_throttle` | `Throttle` | F1 raw는 `0.0..1.0`; percent 표시 가능 |
| brake | `m_brake` | `Brake` | F1 raw는 `0.0..1.0`; percent 표시 가능 |
| steering | `m_steer` | MOZA F1 표에 명확한 key 없음 | 로컬 HUD/logging 핵심 지표 |
| clutch | `m_clutch` | `Clutch` | 직접 대응 |
| gear | `m_gear` | `Gear` | `-1=R`, `0=N`, `1..8` label 변환 |
| RPM | `m_engineRPM` | `Rpm` | 직접 대응 |
| max RPM | `PacketCarStatusData.m_maxRPM` | `MaxRpm` | 직접 대응 |
| REV percent | `m_revLightsPercent` | `CarSettings_CurrentDisplayedRPMPercent`로 추정 | Pit House가 이 key를 F1에서 어떤 raw로 채우는지 실차 검증 필요 |
| REV LED bits | `m_revLightsBitValue` | 직접 key 없음 | 로컬 HUD LED 또는 별도 wheel LED 제어용. Pit House key로는 보존 어려움 |
| DRS active | `m_drs` | `Drs` | 직접 대응 |
| DRS allowed | `PacketCarStatusData.m_drsAllowed` | `DRSAllowed` | 직접 대응 |
| DRS available distance | `PacketCarStatusData.m_drsActivationDistance` | `DRSAvailable`로 파생 가능 | `> 0`이면 available, 값 자체는 남은 거리 |
| DRS fault | `PacketCarDamageData.m_drsFault` | 직접 key 없음 | damage/status HUD용 |

### Fuel, ERS, car status

| 표시/분석 지표 | F1 25 raw field | MOZA F1 key 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| fuel mass | `PacketCarStatusData.m_carStatusData[playerCarIndex].m_fuelInTank` | `FuelRemain`, `Fuel` | 직접 대응 가능. 단위 표기 확인 필요 |
| fuel capacity | `m_fuelCapacity` | `FuelCapacity` | 직접 대응 |
| fuel laps | `m_fuelRemainingLaps` | `FuelRemainLaps`, `FuelSurplusLaps` | 직접/파생 대응 |
| fuel class | `m_fuelRemainingLaps`, race target delta | `FuelClass` | label/color 파생 |
| fuel mix | `m_fuelMix` | `ECUMap`은 F1 24 컬럼 미지원 | F1 25 현대 F1에서는 dashboard 핵심값으로 쓰기 애매함 |
| brake bias | `m_frontBrakeBias` | `BrakeBias` | 직접 대응 |
| pit limiter | `m_pitLimiterStatus` | `PitLimiter` | 직접 대응 |
| traction control | `m_tractionControl` | `TCLevel` | `0=off`, `1=medium`, `2=full` |
| ABS | `m_antiLockBrakes` | `ABSLevel` | `0=off`, `1=on` |
| tyre compound | `m_actualTyreCompound`, `m_visualTyreCompound` | 직접 key 없음 | compound label/color는 local HUD/report용 |
| tyre age | `m_tyresAgeLaps` | 직접 key 없음 | stint/strategy HUD용 |
| ERS energy store | `m_ersStoreEnergy` | `EnergyRemain`, `Ers`, `ERSStored`, `ERSPercent` | Joules raw. percent는 `4,000,000J` 기준 파생 |
| ERS max | F1 rule constant | `ERSMax` | 보통 `4,000,000J` |
| ERS deploy mode | `m_ersDeployMode` | 직접 key 없음 | `0=none`, `1=medium`, `2=hotlap`, `3=overtake` label 변환 |
| ERS deployed this lap | `m_ersDeployedThisLap` | `EnergyDeployed` | 직접 대응 |
| ERS harvested this lap | `m_ersHarvestedThisLapMGUK`, `m_ersHarvestedThisLapMGUH` | `EnergyHarvested` | 두 값을 합산 |
| ERS fault | `PacketCarDamageData.m_ersFault` | 직접 key 없음 | damage/status HUD용 |
| engine power | `m_enginePowerICE`, `m_enginePowerMGUK` | 직접 key 없음 | 분석/로그용. F1 telemetry privacy 설정에 영향 받음 |

### Session, flags, weather

| 표시/분석 지표 | F1 25 raw field | MOZA F1 key 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| track id/name | `PacketSessionData.m_trackId` | `TrackId`, `MapName` | id는 직접, name은 lookup table 필요 |
| track length | `m_trackLength` | `TrackLength` | 직접 대응 |
| session type | `m_sessionType` | `SessionTypeName` | enum label 변환 |
| total laps | `m_totalLaps` | `LapCount` | 직접 대응 |
| session time left | `m_sessionTimeLeft` | `SessionTimeLeft` | 직접 대응 |
| air/track temp | `m_airTemperature`, `m_trackTemperature` | `AirTemp`, `TrackTemp`, Fahrenheit variants | 직접/단위 변환 |
| weather | `m_weather` | 직접 key 없음 | weather label/icon은 local HUD용 |
| forecast/rain chance | `m_weatherForecastSamples[]`, `m_rainPercentage` | 직접 key 없음 | strategy/report용 |
| marshal zones | `m_marshalZones[]` | `YellowFlag`, `GreenFlag` 등으로 파생 가능 | car 위치와 zone을 조합해야 정확함 |
| vehicle FIA flag | `PacketCarStatusData.m_vehicleFiaFlags` | `YellowFlag`, `GreenFlag` | `-1=unknown`, `0=none`, `1=green`, `2=blue`, `3=yellow` |
| blue/red/black/white flags | FIA flags, events, result status 조합 | MOZA F1 컬럼에서 일부 미지원 | 별도 검증 전에는 local HUD/report용 |
| safety car | `m_safetyCarStatus`, `m_numSafetyCarPeriods`, `m_numVirtualSafetyCarPeriods` | 직접 key 없음 | race-control HUD용 |
| spectating | `m_isSpectating` | `Spectating`은 F1 24 컬럼 미지원 | local state 표시 |

### Damage, wear, reliability

| 표시/분석 지표 | F1 25 raw field | MOZA F1 key 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| tyre wear | `PacketCarDamageData.m_tyresWear[4]` | `TyreWearFL/FR/RL/RR` | wheel index 매핑 필요. 현재 packet-level remap 구현됨 |
| tyre damage | `m_tyresDamage[4]` | 직접 key 없음 | local HUD/report용 |
| brake damage | `m_brakesDamage[4]` | 직접 key 없음 | local HUD/report용 |
| tyre blisters | `m_tyreBlisters[4]` | 직접 key 없음 | F1 25 신규/세부 damage 지표. local HUD/report용 |
| front wing damage | `m_frontLeftWingDamage`, `m_frontRightWingDamage` | `WingWearFL`, `WingWearFR` | 직접 대응 |
| rear wing damage | `m_rearWingDamage` | `WingWearR`는 F1 24 컬럼 미지원 | local HUD/report용 |
| floor/diffuser/sidepod damage | `m_floorDamage`, `m_diffuserDamage`, `m_sidepodDamage` | 직접 key 없음 | local damage panel용 |
| gearbox damage | `m_gearBoxDamage` | `GearBoxWear` | 직접 대응 |
| engine damage | `m_engineDamage` | `EngineWear` | 직접 대응 가능 |
| engine component wear | `m_engineMGUHWear`, `m_engineESWear`, `m_engineCEWear`, `m_engineICEWear`, `m_engineMGUKWear`, `m_engineTCWear` | 직접 key 없음 | reliability report용 |
| engine blown/seized | `m_engineBlown`, `m_engineSeized` | 직접 key 없음 | alert용 |

### Setup, strategy, and analysis-only data

| 표시/분석 지표 | F1 25 raw field | MOZA F1 key 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| aero setup | `PacketCarSetupData.m_frontWing`, `m_rearWing` | 직접 key 없음 | setup report, A/B 비교용 |
| differential | `m_onThrottle`, `m_offThrottle` | 직접 key 없음 | traction/rotation recommendation 근거 |
| suspension geometry | `m_frontCamber`, `m_rearCamber`, `m_frontToe`, `m_rearToe` | 직접 key 없음 | tyre temp/wear 분석과 결합 |
| suspension/ARB/ride height | `m_frontSuspension`, `m_rearSuspension`, `m_frontAntiRollBar`, `m_rearAntiRollBar`, `m_frontSuspensionHeight`, `m_rearSuspensionHeight` | 직접 key 없음 | handling recommendation 근거 |
| brake pressure/bias | `m_brakePressure`, `m_brakeBias` | `BrakeBias` 일부 대응 | pressure는 local setup report용 |
| setup tyre pressure | `m_rearLeftTyrePressure`, `m_rearRightTyrePressure`, `m_frontLeftTyrePressure`, `m_frontRightTyrePressure` | running pressure key와 구분 필요 | setup value와 live `m_tyresPressure[4]`를 분리해서 표시 |
| tyre set availability | `PacketTyreSetsData.m_tyreSetData[]`, `m_fittedIdx` | 직접 key 없음 | strategy/report용 |
| lap/stint history | `PacketSessionHistoryData.m_lapHistoryData[]`, `m_tyreStintsHistoryData[]` | 일부 lap time key만 대응 | stint chart/report용 |
| time trial comparison | `PacketTimeTrialData` datasets | 직접 key 없음 | PB/rival comparison용 |

### Motion, map, physics

| 표시/분석 지표 | F1 25 raw field | MOZA F1 key 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| world position | `PacketMotionData.m_carMotionData[playerCarIndex].m_worldPositionX/Y/Z` | `CarCoordinates01/02/03`, `Location` | 축 이름과 표시 정책 필요 |
| velocity | `m_worldVelocityX/Y/Z`, `PacketMotionExData.m_localVelocityX/Y/Z` | `SpeedMs`와 일부 관련 | local physics/logging용 |
| G-force | `m_gForceLateral`, `m_gForceLongitudinal`, `m_gForceVertical` | `GlobalAccelerationG`는 F1 24 컬럼 미지원 | local HUD/analysis용 |
| orientation | `m_yaw`, `m_pitch`, `m_roll` | `Heading`, `Pitch`, `Roll` | radians to display 변환 |
| front wheels angle | `PacketMotionExData.m_frontWheelsAngle` | 직접 key 없음 | steering/understeer 분석용 |
| wheel slip/spin | `m_wheelSlipRatio[4]`, `m_wheelSlipAngle[4]`, `m_wheelSpeed[4]` | `WheelSpin` 파생 가능 | traction/lockup analysis용 |
| wheel forces | `m_wheelLatForce[4]`, `m_wheelLongForce[4]`, `m_wheelVertForce[4]` | 직접 key 없음 | advanced analysis용 |
| aero height/roll/chassis | `m_frontAeroHeight`, `m_rearAeroHeight`, `m_frontRollAngle`, `m_rearRollAngle`, `m_chassisYaw`, `m_chassisPitch` | 일부 `Pitch/Roll`과 관련 | setup analysis용 |

## 현재 브리지 parser coverage

| F1 packet | 현재 코드 상태 | 빠진 주요 지표 |
| --- | --- | --- |
| `PacketSessionData` | 일부 파싱: total laps, track length, session type, track id | weather, forecast, safety car, marshal zones, session time left |
| `PacketLapData` | 일부 파싱: lap time, gap to front/leader, lap distance, position, lap, pit, sector, invalid, driver/result status | penalties, warnings, pit timer, speed trap |
| `PacketCarTelemetryData` | 일부 파싱: throttle/brake/steer/clutch/speed/gear/RPM/DRS/REV/temps/pressure | surface type, suggested gear/MFD panel |
| `PacketCarStatusData` | 일부 파싱: assists, brake bias, fuel, RPM limits, DRS, tyre compound/age, ERS | DRS activation distance, FIA flags, engine power, network paused |
| `PacketCarDamageData` | 일부 파싱: tyre wear/damage/blisters, wing damage | brake damage, floor/diffuser/sidepod, faults, component wear |
| `PacketMotionData` | 미구현 | position, velocity, G-force, yaw/pitch/roll |
| `PacketCarSetupData` | 미구현 | setup recommendation/report에 필요 |
| `PacketParticipantsData` | 미구현 | player/team/name/car identity |
| `PacketEventData` | 미구현 | penalties, flags, speed trap, collisions, overtake events |
| `PacketSessionHistoryData` | 미구현 | best lap/stint history |
| `PacketTyreSetsData` | 미구현 | tyre set strategy |
| `PacketMotionExData` | 미구현 | slip/forces/aero height/advanced physics |

## Multiplayer telemetry visibility

F1 25의 `Your Telemetry` 설정이 `Restricted`인 경우, 다른 플레이어 차량의 일부 값은 UDP에서 0으로 내려올 수 있습니다. 플레이어 본인 차량은 항상 볼 수 있지만, 상대 차량 분석이나 leaderboard gap 외 세부 상태 분석에서는 이 제한을 고려해야 합니다.

제한 대상이 될 수 있는 대표 field:

| Packet | Restricted 때 0 처리될 수 있는 값 |
| --- | --- |
| `PacketCarStatusData` | `m_fuelInTank`, `m_fuelCapacity`, `m_fuelMix`, `m_fuelRemainingLaps`, `m_frontBrakeBias`, `m_ersDeployMode`, `m_ersStoreEnergy`, `m_ersDeployedThisLap`, `m_ersHarvestedThisLapMGUK`, `m_ersHarvestedThisLapMGUH`, `m_enginePowerICE`, `m_enginePowerMGUK` |
| `PacketCarDamageData` | wing/floor/diffuser/sidepod damage, `m_engineDamage`, `m_gearBoxDamage`, `m_tyresWear[4]`, `m_tyresDamage[4]`, `m_brakesDamage[4]`, DRS fault, engine component wear |

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
