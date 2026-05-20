# 확정 텔레메트리 매핑

확인일: 2026-05-19

이 문서는 지금까지 모은 자료와 현재 코드 기준으로 방어 가능한 값만 매핑합니다.

확실성 구분:

- `source+key`: 브리지가 원본 값을 현재 파싱하고, 관련 게임 계열에 대응하는 MOZA `v1/gameData/...` 키가 확인됨
- `derived`: 원본 값은 확실하지만 대시보드 값에는 기계적인 단위 변환 또는 불리언 파생이 필요함
- `local-only`: 원본 값은 확실하지만 현재 확인된 MOZA 키 지원이 없거나 가정하기 위험함
- `output-only`: MOZA 키 지원은 확인됐지만 네이티브 게임 어댑터가 아직 구현되지 않음

브리지는 Pit House에 새 키를 등록할 수 없습니다. 이 매핑은 정규화된 어댑터 대상과 로컬 HUD/로깅 계층에 넣을 수 있는 값을 정의합니다.

## F1 25 원본에서 MOZA 키로 매핑

아래 매핑은 현재 `src/f1/*` 파서 범위와 MOZA Digital Dash 표의 F1 계열 키를 기준으로 합니다.

### 속도, 입력, REV, DRS

| MOZA 키 | F1 25 원본 | 확실성 | 변환 |
| --- | --- | --- | --- |
| `SpeedKmh` | `PacketCarTelemetryData.m_speed` | `source+key` | 없음 |
| `SpeedMph` | `PacketCarTelemetryData.m_speed` | `derived` | `km/h * 0.621371` |
| `SpeedMs` | `PacketCarTelemetryData.m_speed` | `derived` | `km/h / 3.6` |
| `Rpm` | `PacketCarTelemetryData.m_engineRPM` | `source+key` | 없음 |
| `Gear` | `PacketCarTelemetryData.m_gear` | `source+key` | `-1=R`, `0=N`, 양수는 그대로 |
| `Throttle` | `PacketCarTelemetryData.m_throttle` | `source+key` | 정규화된 `0.0..1.0`; UI에서는 퍼센트 표시 가능 |
| `Brake` | `PacketCarTelemetryData.m_brake` | `source+key` | 정규화된 `0.0..1.0`; UI에서는 퍼센트 표시 가능 |
| `Clutch` | `PacketCarTelemetryData.m_clutch` | `source+key` | F1 원본 클러치 값 |
| `Drs` | `PacketCarTelemetryData.m_drs` | `source+key` | `0/1` 또는 불리언 |
| `CarSettings_CurrentDisplayedRPMPercent` | `PacketCarTelemetryData.m_revLightsPercent` | `derived` | F1이 퍼센트를 직접 제공함. 휠 LED 동작은 하드웨어 검증 필요 |

### 타이어, 브레이크, 압력

F1 휠 배열은 항상 `0=RL`, `1=RR`, `2=FL`, `3=FR`입니다. 아래 이름 기반 코너 키는 모두 이 순서를 재매핑한 뒤 채워야 합니다.

| MOZA 키 | F1 25 원본 | 확실성 | 변환 |
| --- | --- | --- | --- |
| `TyreTempFL` | `m_tyresSurfaceTemperature[2]` | `source+key` | 섭씨 |
| `TyreTempFR` | `m_tyresSurfaceTemperature[3]` | `source+key` | 섭씨 |
| `TyreTempRL` | `m_tyresSurfaceTemperature[0]` | `source+key` | 섭씨 |
| `TyreTempRR` | `m_tyresSurfaceTemperature[1]` | `source+key` | 섭씨 |
| `TyreTempFLI` | `m_tyresInnerTemperature[2]` | `source+key` | 섭씨 |
| `TyreTempFRI` | `m_tyresInnerTemperature[3]` | `source+key` | 섭씨 |
| `TyreTempRLI` | `m_tyresInnerTemperature[0]` | `source+key` | 섭씨 |
| `TyreTempRRI` | `m_tyresInnerTemperature[1]` | `source+key` | 섭씨 |
| `TyreTempFL&F`, `TyreTempFR&F`, `TyreTempRL&F`, `TyreTempRR&F` | 위 표면 타이어 온도 | `derived` | `C * 9 / 5 + 32` |
| `TyreTempFLI&F`, `TyreTempFRI&F`, `TyreTempRLI&F`, `TyreTempRRI&F` | 위 내부 타이어 온도 | `derived` | `C * 9 / 5 + 32` |
| `BrakeTempFL` | `m_brakesTemperature[2]` | `source+key` | 섭씨 |
| `BrakeTempFR` | `m_brakesTemperature[3]` | `source+key` | 섭씨 |
| `BrakeTempRL` | `m_brakesTemperature[0]` | `source+key` | 섭씨 |
| `BrakeTempRR` | `m_brakesTemperature[1]` | `source+key` | 섭씨 |
| `BrakeTempFL&F`, `BrakeTempFR&F`, `BrakeTempRL&F`, `BrakeTempRR&F` | 위 브레이크 온도 | `derived` | `C * 9 / 5 + 32` |
| `TyrePressureFL` | `m_tyresPressure[2]` | `source+key` | PSI |
| `TyrePressureFR` | `m_tyresPressure[3]` | `source+key` | PSI |
| `TyrePressureRL` | `m_tyresPressure[0]` | `source+key` | PSI |
| `TyrePressureRR` | `m_tyresPressure[1]` | `source+key` | PSI |
| `TyreWearFL` | `PacketCarDamageData.m_tyresWear[2]` | `source+key` | 퍼센트 |
| `TyreWearFR` | `PacketCarDamageData.m_tyresWear[3]` | `source+key` | 퍼센트 |
| `TyreWearRL` | `PacketCarDamageData.m_tyresWear[0]` | `source+key` | 퍼센트 |
| `TyreWearRR` | `PacketCarDamageData.m_tyresWear[1]` | `source+key` | 퍼센트 |

### 랩, 세션, 순위

| MOZA 키 | F1 25 원본 | 확실성 | 변환 |
| --- | --- | --- | --- |
| `LapCount` | `PacketSessionData.m_totalLaps` | `source+key` | 없음 |
| `TrackLength` | `PacketSessionData.m_trackLength` | `source+key` | 미터 |
| `TrackId` | `PacketSessionData.m_trackId` | `source+key` | 숫자 ID |
| `SessionTimeLeft` | `PacketSessionData.m_sessionTimeLeft` | `source+key` | 초 |
| `TrackTemp` | `PacketSessionData.m_trackTemperature` | `source+key` | 섭씨 |
| `AirTemp` | `PacketSessionData.m_airTemperature` | `source+key` | 섭씨 |
| `TrackTemp&F`, `AirTemp&F` | 위 트랙/공기 온도 | `derived` | `C * 9 / 5 + 32` |
| `Lap` | `PacketLapData.m_currentLapNum` | `source+key` | 없음 |
| `Pos` | `PacketLapData.m_carPosition` | `source+key` | 없음 |
| `LastLapTime` | `PacketLapData.m_lastLapTimeInMS` | `source+key` | 밀리초; UI에서 시간 형식으로 표시 |
| `CurrentLapTime` | `PacketLapData.m_currentLapTimeInMS` | `source+key` | 밀리초; UI에서 시간 형식으로 표시 |
| `SectorIndex` | `PacketLapData.m_sector` | `source+key` | `0=S1`, `1=S2`, `2=S3` |
| `Sector1Time` | `PacketLapData.m_sector1Time*` | `source+key` | 분/밀리초 부분을 밀리초로 결합 |
| `Sector2Time` | `PacketLapData.m_sector2Time*` | `source+key` | 분/밀리초 부분을 밀리초로 결합 |
| `LapInvalidated` | `PacketLapData.m_currentLapInvalid` | `source+key` | 불리언 |
| `IsInPit` | `PacketLapData.m_pitStatus` | `derived` | 피트 상태가 `0`이 아니면 true |
| `Pitlane` | `PacketLapData.m_pitStatus` | `derived` | 피팅 중 또는 피트 구역 안이면 true |
| `PlayerIndex` | `PacketHeader.m_playerCarIndex` | `source+key` | 없음 |
| `TrackPositionPercent` | `m_lapDistance / m_trackLength` | `derived` | `PacketLapData`와 `PacketSessionData` 조합 필요 |

### 상태, 연료, ERS, 보조 장치

| MOZA 키 | F1 25 원본 | 확실성 | 변환 |
| --- | --- | --- | --- |
| `MaxRpm` | `PacketCarStatusData.m_maxRPM` | `source+key` | 없음 |
| `ABSLevel` | `PacketCarStatusData.m_antiLockBrakes` | `source+key` | F1 `0/1` |
| `TCLevel` | `PacketCarStatusData.m_tractionControl` | `source+key` | F1 `0/1/2` |
| `BrakeBias` | `PacketCarStatusData.m_frontBrakeBias` | `source+key` | 퍼센트 |
| `DRSAllowed` | `PacketCarStatusData.m_drsAllowed` | `source+key` | 불리언 |
| `DRSAvailable` | `PacketCarStatusData.m_drsActivationDistance` | `derived` | 거리가 `0`보다 크면 true |
| `PitLimiter` | `PacketCarStatusData.m_pitLimiterStatus` | `source+key` | 불리언 |
| `CarSettings_MaxGears` | `PacketCarStatusData.m_maxGears` | `source+key` | 없음 |
| `FuelRemain` | `PacketCarStatusData.m_fuelInTank` | `source+key` | F1 연료 질량 |
| `Fuel` | `PacketCarStatusData.m_fuelInTank` | `source+key` | `FuelRemain`과 같은 원본 |
| `FuelCapacity` | `PacketCarStatusData.m_fuelCapacity` | `source+key` | F1 연료 질량 용량 |
| `FuelRemainLaps` | `PacketCarStatusData.m_fuelRemainingLaps` | `source+key` | 랩 수 |
| `EnergyRemain`, `Ers`, `ERSStored` | `PacketCarStatusData.m_ersStoreEnergy` | `source+key` | 줄 |
| `ERSPercent` | `PacketCarStatusData.m_ersStoreEnergy` | `derived` | `ersStoreEnergy / 4_000_000 * 100` |
| `ERSMax` | F1 규칙 상수 | `derived` | `4_000_000J` |
| `EnergyDeployed` | `PacketCarStatusData.m_ersDeployedThisLap` | `source+key` | 줄 |

### 손상

| MOZA 키 | F1 25 원본 | 확실성 | 변환 |
| --- | --- | --- | --- |
| `WingWearFL` | `PacketCarDamageData.m_frontLeftWingDamage` | `source+key` | 퍼센트 |
| `WingWearFR` | `PacketCarDamageData.m_frontRightWingDamage` | `source+key` | 퍼센트 |
| `GearBoxWear` | `PacketCarDamageData.m_gearBoxDamage` | `source+key` | 퍼센트 |
| `EngineWear` | `PacketCarDamageData.m_engineDamage` | `source+key` | 퍼센트 |

## F1 25 로컬 전용 확정 필드

아래 값은 브리지의 정규화 모델, HUD, 로깅, 분석 안에서는 안전하게 매핑할 수 있습니다. 다만 일치하는 MOZA Pit House 키 지원이 확인되기 전에는 Pit House 키로 보내면 안 됩니다.

| 로컬 필드 | F1 25 원본 | 로컬 전용 이유 |
| --- | --- | --- |
| `frontGapMs` | `m_deltaToCarInFrontMinutesPart`, `m_deltaToCarInFrontMSPart` | 수집한 표에서 MOZA `Gap`은 F1-family 지원으로 표시되지 않음 |
| `behindGapMs` | 바로 뒤 순위 차량의 `m_deltaToCarInFrontMinutesPart`, `m_deltaToCarInFrontMSPart` | 확정된 Pit House 키 없음. 로컬 HUD에서 player 기준 뒤차 gap으로 파생 |
| `leaderGapMs` | `m_deltaToRaceLeaderMinutesPart`, `m_deltaToRaceLeaderMSPart` | 확정된 `LeaderGap`/`BehindGap` Pit House 키 없음 |
| `steer` | `PacketCarTelemetryData.m_steer` | 확정된 MOZA F1 대시보드 키 없음 |
| `revLightsBitValue` | `PacketCarTelemetryData.m_revLightsBitValue` | 로컬/휠 LED에는 유용하지만 확정 MOZA 키 없음 |
| `tyreDamageFL/FR/RL/RR` | `PacketCarDamageData.m_tyresDamage[4]` | 확정 MOZA 키 없음 |
| `tyreBlistersFL/FR/RL/RR` | `PacketCarDamageData.m_tyreBlisters[4]` | 확정 MOZA 키 없음 |
| `rearWingDamage` | `PacketCarDamageData.m_rearWingDamage` | `WingWearR`는 전역 키로 존재하지만 F1 계열 지원이 확인되지 않음 |

## F1 25 원본은 있지만 정책이 필요한 키

아래 값은 그럴듯한 F1 원본과 MOZA 키 이름이 있지만, 실시간 매핑 값으로 취급하기 전에 프로젝트 정책이 필요합니다.

| MOZA 키 | F1 25 원본 | 필요한 결정 |
| --- | --- | --- |
| `FuelSurplusLaps` | `PacketCarStatusData.m_fuelRemainingLaps`와 레이스 목표 정책 | 단순 남은 랩 수인지, 레이스 목표 대비 여유분인지 정의 필요 |

## LU / LMU 출력 매핑 대상

LU/LMU 원본 어댑터는 아직 구현되지 않았으므로 원본 필드는 여기서 매핑하지 않습니다. 아래 키는 `Le mans ultimate` 컬럼에서 지원이 확인되었으므로 실시간 어댑터가 생기면 첫 출력 대상으로 삼습니다.

| 그룹 | 확인된 LU/LMU MOZA 키 |
| --- | --- |
| 랩/레이스 | `MaxRpm`, `LapCount`, `CarCount`, `Lap`, `Pos`, `Gap`, `EstimatedLapTime`, `LastLapTime`, `BestLapTime`, `CurrentLapTime`, `CompletedLaps`, `LapInvalidated`, `SessionTypeName`, `PlayerIndex`, `OpponentCount` |
| 속도/입력 | `SpeedMph`, `SpeedKmh`, `SpeedMs`, `Rpm`, `Gear`, `Throttle`, `Brake`, `Clutch`, `BrakeBias`, `Boost`, `CarSettings_MaxGears`, `CarSettings_CurrentDisplayedRPMPercent`, `WheelSpin` |
| 연료 | `FuelRemain`, `FuelCapacity`, `FuelTemp`, `Fuel` |
| 타이어 | `TyreWearFL`, `TyreWearFR`, `TyreWearRL`, `TyreWearRR`, `TyrePressureFL`, `TyrePressureFR`, `TyrePressureRL`, `TyrePressureRR`, `TyreTempFL`, `TyreTempFR`, `TyreTempRL`, `TyreTempRR`, `TyreTempFLI`, `TyreTempFRI`, `TyreTempRLI`, `TyreTempRRI`, `TyreTempFLM`, `TyreTempFRM`, `TyreTempRLM`, `TyreTempRRM`, `TyreTempFLO`, `TyreTempFRO`, `TyreTempRL0`, `TyreTempRRO` |
| 화씨 타이어 온도 | `TyreTempFL&F`, `TyreTempFLI&F`, `TyreTempFLO&F`, `TyreTempFR&F`, `TyreTempFRI&F`, `TyreTempFRO&F`, `TyreTempRL&F`, `TyreTempRLI&F`, `TyreTempRLO&F`, `TyreTempRR&F`, `TyreTempRRI&F`, `TyreTempRRO&F` |
| 브레이크 | `BrakeTempFL`, `BrakeTempFR`, `BrakeTempRL`, `BrakeTempRR`, `BrakeTempFL&F`, `BrakeTempFR&F`, `BrakeTempRL&F`, `BrakeTempRR&F` |
| 환경 | `TrackTemp`, `AirTemp`, `TrackTemp&F`, `AirTemp&F`, `WaterTemperature` |
| 피트/플래그/세션 | `IsInPit`, `EngineIgnition`, `Pitlane`, `PitLimiter`, `YellowFlag`, `SectorIndex`, `Sector1Time`, `Sector2Time`, `EngineStarted`, `SectorsCount` |
| 모션/맵 | `Pitch`, `Roll`, `MapName`, `CarCoordinates01`, `CarCoordinates02`, `CarCoordinates03`, `TrackLength`, `TrackId`, `TrackPositionPercent`, `Location` |
| 식별 정보 | `CarId`, `PlayerName`, `CarModel`, `Gamename` |

## ACE / ACR 매핑 경계

ACE와 ACR은 아직 Pit House key로 매핑하지 않습니다.

확정된 사실:

- MOZA 게임 호환 목록은 두 게임 모두 텔레메트리 지원으로 표시합니다.
- Digital Dash 키 매트릭스에는 `Assetto Corsa EVO` 또는 `Assetto Corsa Rally` 전용 컬럼이 없습니다.
- ACE는 공유 메모리 텔레메트리를 가리키고, ACR은 네이티브/보조 리더 경로를 가리킵니다.

따라서:

- ACE/ACR의 정확한 `v1/gameData/...` 키 범위를 주장하지 않습니다.
- 원본 필드가 검증되면 향후 어댑터는 F1/LU와 같은 정규화 핵심 필드로 매핑합니다: 속도, RPM, 기어, 스로틀, 브레이크, 클러치, 조향, 타이어 온도, 타이어 압력, 노출된다면 타이어 웨어, 브레이크 온도, 랩 타이밍, 노출된다면 차간/타이밍, 노출된다면 트랙 위치.
- 불확실한 ACE/ACR 필드는 실제 Pit House 캡처로 대시보드 키가 확인되기 전까지 로컬 HUD/로깅 계층에 둡니다.

## 구현 우선순위

1. 위 F1 파싱 값을 로컬 HUD/API용 정규화 `gameData` 맵으로 연결합니다.
2. F1 `Gap`은 Pit House에서 F1 지원 키가 확인되기 전까지 `local-only`로 유지합니다.
3. LU/LMU는 MOZA 키 지원이 확인되어 있으므로 `output-only` 키 그룹을 먼저 어댑터 대상으로 구현합니다.
4. ACE/ACR은 공유 메모리 구조체 이름이나 필드 오프셋을 하드코딩하기 전에 탐지 절차를 먼저 추가합니다.
