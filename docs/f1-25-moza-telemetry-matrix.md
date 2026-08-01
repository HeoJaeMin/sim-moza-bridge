# F1 25 / MOZA 텔레메트리 매트릭스

이 문서는 F1 25 UDP 텔레메트리 필드와 MOZA Pit House Digital Dash 텔레메트리 키를 비교한 작업 기준표입니다.

## 기준

- 확인일: 2026-07-30
- F1 25 기준: F1 25 기본 UDP 형식 `2025` 및 2026 Season Pack UDP 형식 `2026`
- MOZA 기준: MOZA Racing Support Center의 `Digital Dash Telemetry Support` 표
- 주의: MOZA 공식 표에는 `F1 25` 컬럼이 없고 `F1 24`까지만 있습니다. 그래서 F1 계열 Pit House 키 지원 여부는 `F1 24` 컬럼을 기준으로 비교했습니다.

참고 원문:

- [MOZA Digital Dash Telemetry Support](https://support.mozaracing.com/en/support/solutions/articles/70000627978-digital-dash-telemetry-support)
- [EA F1 25 2026 Season Pack UDP specification](https://forums.ea.com/blog/f1-games-game-info-hub-en/ea-sports%E2%84%A2-f1%C2%AE25-2026-season-pack-udp-specification/12187347)
- [F1 25 UDP structure mirror used for field extraction](https://github.com/MacManley/f1-25-udp)

## 결론

MOZA 표에는 전체 텔레메트리 이름이 144개 있고, 그중 `F1 24` 컬럼에 지원 표시가 있는 키는 109개입니다. F1 25 UDP에는 그보다 훨씬 많은 원본 필드가 있으며, 여러 값은 단위 변환, 열거값 라벨 변환, 휠 배열 순서 보정, 플레이어 차량 선택, 또는 여러 패킷 조합이 필요합니다. 대표적으로 차간 간격은 F1 25 UDP에 `m_deltaToCarInFront*`, `m_deltaToRaceLeader*`로 존재하지만 MOZA F1 계열 키로는 지원 표시가 없습니다.

현재 브리지가 MOZA로 전달되는 패킷에서 실제로 수정하는 값은 `PacketCarDamageData.m_tyresWear[4]`이며, F1 25 CarDamage 패킷은 기본적으로 F1 24 호환 레이아웃으로 줄여 전달합니다. 나머지는 로컬 HUD, 로깅, 분석에서 읽거나 파생할 수 있지만 Pit House에 새 `v1/gameData/...` 키를 등록하지는 못합니다.

## F1 원본 휠 배열 규칙

F1 25 UDP 문서의 실제 타이어 웨어 필드명은 `typeWear`가 아니라 `m_tyresWear[4]`입니다. `PacketCarDamageData`는 22대 차량의 `CarDamageData` 배열을 들고 있으므로 플레이어 차량의 타이어 웨어는 다음 경로로 읽습니다.

```text
PacketCarDamageData.m_carDamageData[PacketHeader.m_playerCarIndex].m_tyresWear[index]
```

F1 25의 모든 휠 배열은 같은 인덱스 순서를 씁니다.

| F1 배열 인덱스 | 위치 | 약어 |
| --- | --- | --- |
| `0` | 뒤 왼쪽 | `RL` |
| `1` | 뒤 오른쪽 | `RR` |
| `2` | 앞 왼쪽 | `FL` |
| `3` | 앞 오른쪽 | `FR` |

따라서 MOZA 이름 기준 키로 표현하려면 인덱스를 그대로 쓰면 안 되고 아래처럼 코너 이름으로 다시 매핑해야 합니다.

| MOZA 키 | F1 원본 필드 경로 |
| --- | --- |
| `TyreWearFL` | `m_carDamageData[playerCarIndex].m_tyresWear[2]` |
| `TyreWearFR` | `m_carDamageData[playerCarIndex].m_tyresWear[3]` |
| `TyreWearRL` | `m_carDamageData[playerCarIndex].m_tyresWear[0]` |
| `TyreWearRR` | `m_carDamageData[playerCarIndex].m_tyresWear[1]` |

브리지는 기본 실행에서 F1 25 CarDamage 패킷을 F1 24 호환 레이아웃으로 변환합니다. Pit House가 이미 F1 사양대로 `TyreWearFL = m_tyresWear[2]`로 읽는다는 전제에 맞춰, 휠 순서 자체는 원본 F1 배열을 유지합니다.
값이 `100`에서 움직이지 않는 경우는 순서 문제가 아니라 Pit House가 F1 25 CarDamage 패킷을 읽지 못하는 호환성 문제일 가능성이 큽니다.

이 인덱스 규칙은 타이어 웨어만의 문제가 아닙니다. F1 25에서 아래 휠 배열들도 같은 `0=RL, 1=RR, 2=FL, 3=FR` 순서를 씁니다.

| F1 패킷 | F1 필드 | MOZA 이름 기준으로 매핑이 필요한 키 |
| --- | --- | --- |
| `PacketCarDamageData` | `m_tyresWear[4]` | `TyreWearFL`, `TyreWearFR`, `TyreWearRL`, `TyreWearRR` |
| `PacketCarDamageData` | `m_tyresDamage[4]` | 직접 대응 키는 MOZA F1 계열 표에 없음. 로컬 HUD/리포트에서는 코너 이름으로 매핑 필요 |
| `PacketCarDamageData` | `m_brakesDamage[4]` | 직접 대응 키는 MOZA F1 계열 표에 없음. 로컬 HUD/리포트에서는 코너 이름으로 매핑 필요 |
| `PacketCarDamageData` | `m_tyreBlisters[4]` | 직접 대응 키는 MOZA F1 계열 표에 없음. 로컬 HUD/리포트에서는 코너 이름으로 매핑 필요 |
| `PacketCarTelemetryData` | `m_brakesTemperature[4]` | `BrakeTempFL`, `BrakeTempFR`, `BrakeTempRL`, `BrakeTempRR` |
| `PacketCarTelemetryData` | `m_tyresSurfaceTemperature[4]` | `TyreTempFL`, `TyreTempFR`, `TyreTempRL`, `TyreTempRR` |
| `PacketCarTelemetryData` | `m_tyresInnerTemperature[4]` | `TyreTempFLI`, `TyreTempFRI`, `TyreTempRLI`, `TyreTempRRI` |
| `PacketCarTelemetryData` | `m_tyresPressure[4]` | `TyrePressureFL`, `TyrePressureFR`, `TyrePressureRL`, `TyrePressureRR` |
| `PacketMotionExData` | `m_wheelSpeed[4]`, `m_wheelSlipRatio[4]`, `m_wheelSlipAngle[4]`, `m_wheelLatForce[4]`, `m_wheelLongForce[4]`, `m_wheelVertForce[4]`, `m_wheelCamber[4]`, `m_wheelCamberGain[4]` | Pit House F1 계열 키로 직접 보존하기 어렵고, 로컬 분석/HUD에서 코너 이름으로 매핑해야 함 |

## 주요 F1 원본 지표 상세 매핑

아래 표는 대시보드나 분석에 쓰기 쉬운 단위로 F1 25 원본 필드를 다시 묶은 것입니다. `m_*Data[playerCarIndex]`라고 적힌 항목은 패킷 헤더의 `m_playerCarIndex`로 플레이어 차량 행을 골라야 합니다.

### 차간 간격, 랩, 순위

| 표시/분석 지표 | F1 25 원본 필드 | MOZA F1 키 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| 앞차와의 차이 | `PacketLapData.m_lapData[playerCarIndex].m_deltaToCarInFrontMinutesPart`, `m_deltaToCarInFrontMSPart` | MOZA 표의 `Gap`은 F1 24 컬럼 미지원. `FrontGap` 같은 키도 표에 없음 | `minutes * 60000 + ms`로 밀리초 차이를 생성. `minutes == 255`는 무효값으로 처리. 로컬 HUD/리포트용 |
| 뒤차와의 차이 | 바로 뒤 순위 차량의 `m_deltaToCarInFrontMinutesPart`, `m_deltaToCarInFrontMSPart` | 직접 키 없음 | 플레이어보다 한 순위 뒤 차량의 앞차 gap을 player 기준 뒤차 gap으로 파생. 로컬 HUD용 |
| 선두와의 차이 | `PacketLapData.m_lapData[playerCarIndex].m_deltaToRaceLeaderMinutesPart`, `m_deltaToRaceLeaderMSPart` | 직접 키 없음 | 선두와의 차이로 별도 표시. MOZA Pit House 키 주입은 불가 |
| 세이프티카 델타 | `PacketLapData.m_lapData[playerCarIndex].m_safetyCarDelta` | 직접 키 없음 | 세이프티카/VSC HUD 또는 분석용 |
| 현재 순위 | `m_lapData[playerCarIndex].m_carPosition` | `Pos` | 직접 대응 |
| 현재 랩 | `m_lapData[playerCarIndex].m_currentLapNum` | `Lap` | 직접 대응. 완료 랩 수는 현재 랩에서 파생 |
| 완료 랩 수 | `m_currentLapNum`, 시작/결승선 통과 상태 | `CompletedLaps` | F1 원본에 그대로 있는 값이 아니라 랩 상태에서 파생 |
| 현재 랩 타임 | `m_currentLapTimeInMS` | `CurrentLapTime` | 밀리초를 표시 형식으로 변환 |
| 직전 랩 타임 | `m_lastLapTimeInMS` | `LastLapTime` | 밀리초를 표시 형식으로 변환 |
| 베스트 랩 타임 | `PacketSessionHistoryData.m_lapHistoryData[].m_lapTimeInMS` 또는 `PacketFinalClassificationData.m_bestLapTimeInMS` | `BestLapTime` | 세션 히스토리/최종 분류 파서 필요 |
| 섹터 인덱스 | `m_lapData[playerCarIndex].m_sector` | `SectorIndex` | `0=S1`, `1=S2`, `2=S3` 라벨 변환 |
| 섹터 1/2 타임 | `m_sector1TimeMinutesPart + m_sector1TimeMSPart`, `m_sector2TimeMinutesPart + m_sector2TimeMSPart` | `Sector1Time`, `Sector2Time` | 분/밀리초 조합 변환 |
| 랩 무효 여부 | `m_currentLapInvalid` | `LapInvalidated` | 직접 대응 |
| 페널티/경고 | `m_penalties`, `m_totalWarnings`, `m_cornerCuttingWarnings` | 직접 키 없음 | 로컬 HUD/리포트용 |
| 피트 상태 | `m_pitStatus` | `IsInPit`, `Pitlane` | `0=없음`, `1=피팅 중`, `2=피트 구역 안` 변환 |
| 피트 스톱 타이밍 | `m_pitLaneTimerActive`, `m_pitLaneTimeInLaneInMS`, `m_pitStopTimerInMS`, `m_pitStopShouldServePen` | 제한적 | 피트 HUD/리포트용. MOZA 표 직접 키 부족 |
| 드라이버/결과 상태 | `m_driverStatus`, `m_resultStatus` | 제한적 | 열거값 라벨 변환. 비활성/리타이어 필터링에 필요 |
| 트랙 위치 퍼센트 | `m_lapDistance / PacketSessionData.m_trackLength` | `TrackPositionPercent` | 파생 가능 |

### 속도, 입력, REV, DRS

| 표시/분석 지표 | F1 25 원본 필드 | MOZA F1 키 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| 속도 km/h | `PacketCarTelemetryData.m_carTelemetryData[playerCarIndex].m_speed` | `SpeedKmh` | 직접 대응 |
| 속도 mph / m/s | `m_speed` | `SpeedMph`, `SpeedMs` | 단위 변환 |
| 스로틀 | `m_throttle` | `Throttle` | F1 원본은 `0.0..1.0`; 퍼센트 표시 가능 |
| 브레이크 | `m_brake` | `Brake` | F1 원본은 `0.0..1.0`; 퍼센트 표시 가능 |
| 조향 | `m_steer` | MOZA F1 표에 명확한 키 없음 | 로컬 HUD/로깅 핵심 지표 |
| clutch | `m_clutch` | `Clutch` | 직접 대응 |
| 기어 | `m_gear` | `Gear` | `-1=R`, `0=N`, `1..8` 라벨 변환 |
| RPM | `m_engineRPM` | `Rpm` | 직접 대응 |
| 최대 RPM | `PacketCarStatusData.m_maxRPM` | `MaxRpm` | 직접 대응 |
| REV 퍼센트 | `m_revLightsPercent` | `CarSettings_CurrentDisplayedRPMPercent`로 추정 | Pit House가 이 키를 F1에서 어떤 원본 값으로 채우는지 실차 검증 필요 |
| REV LED 비트 | `m_revLightsBitValue` | 직접 키 없음 | 로컬 HUD LED 또는 별도 휠 LED 제어용. Pit House 키로는 보존 어려움 |
| DRS 활성 | `m_drs` | `Drs` | 직접 대응 |
| DRS allowed | `PacketCarStatusData.m_drsAllowed` | `DRSAllowed` | 직접 대응 |
| DRS 사용 가능 거리 | `PacketCarStatusData.m_drsActivationDistance` | `DRSAvailable`로 파생 가능 | `> 0`이면 사용 가능, 값 자체는 남은 거리 |
| DRS 고장 | `PacketCarDamageData.m_drsFault` | 직접 키 없음 | 손상/상태 HUD용 |

### 연료, ERS, 차량 상태

| 표시/분석 지표 | F1 25 원본 필드 | MOZA F1 키 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| 연료 질량 | `PacketCarStatusData.m_carStatusData[playerCarIndex].m_fuelInTank` | `FuelRemain`, `Fuel` | 직접 대응 가능. 단위 표기 확인 필요 |
| 연료 용량 | `m_fuelCapacity` | `FuelCapacity` | 직접 대응 |
| 연료 잔여 랩 | `m_fuelRemainingLaps` | `FuelRemainLaps`, `FuelSurplusLaps` | 직접/파생 대응 |
| 연료 등급 | `m_fuelRemainingLaps`, 레이스 목표 차이 | `FuelClass` | 라벨/색상 파생 |
| 연료 믹스 | `m_fuelMix` | `ECUMap`은 F1 24 컬럼 미지원 | F1 25 현대 F1에서는 대시보드 핵심값으로 쓰기 애매함 |
| 브레이크 바이어스 | `m_frontBrakeBias` | `BrakeBias` | 직접 대응 |
| 피트 리미터 | `m_pitLimiterStatus` | `PitLimiter` | 직접 대응 |
| 트랙션 컨트롤 | `m_tractionControl` | `TCLevel` | `0=꺼짐`, `1=중간`, `2=최대` |
| ABS | `m_antiLockBrakes` | `ABSLevel` | `0=off`, `1=on` |
| 타이어 컴파운드 | `m_actualTyreCompound`, `m_visualTyreCompound` | 직접 키 없음 | 컴파운드 라벨/색상은 로컬 HUD/리포트용 |
| 타이어 사용 랩 수 | `m_tyresAgeLaps` | 직접 키 없음 | 스틴트/전략 HUD용 |
| ERS 저장 에너지 | `m_ersStoreEnergy` | `EnergyRemain`, `Ers`, `ERSStored`, `ERSPercent` | 원본 단위는 줄. 퍼센트는 `4,000,000J` 기준 파생 |
| ERS max | F1 rule constant | `ERSMax` | 보통 `4,000,000J` |
| ERS 배포 모드 | `m_ersDeployMode` | 직접 키 없음 | `0=없음`, `1=중간`, `2=핫랩`, `3=오버테이크` 라벨 변환 |
| ERS deployed this lap | `m_ersDeployedThisLap` | `EnergyDeployed` | 직접 대응 |
| ERS harvested this lap | `m_ersHarvestedThisLapMGUK`, `m_ersHarvestedThisLapMGUH` | `EnergyHarvested` | 두 값을 합산 |
| 2026 랩당 ERS 회수 한도 | `m_ersHarvestedLimitPerLap` | 직접 키 없음 | 2026 Season Pack 전용. MGU-K/H 회수량과 함께 원본 줄 단위로 기록 |
| ERS 고장 | `PacketCarDamageData.m_ersFault` | 직접 키 없음 | 손상/상태 HUD용 |
| 엔진 출력 | `m_enginePowerICE`, `m_enginePowerMGUK` | 직접 키 없음 | 분석/로그용. F1 텔레메트리 공개 설정에 영향 받음 |

### 세션, 플래그, 날씨

| 표시/분석 지표 | F1 25 원본 필드 | MOZA F1 키 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| 트랙 ID/이름 | `PacketSessionData.m_trackId` | `TrackId`, `MapName` | ID는 직접, 이름은 조회표 필요 |
| 트랙 길이 | `m_trackLength` | `TrackLength` | 직접 대응 |
| 세션 유형 | `m_sessionType` | `SessionTypeName` | 열거값 라벨 변환 |
| 총 랩 수 | `m_totalLaps` | `LapCount` | 직접 대응 |
| 남은 세션 시간 | `m_sessionTimeLeft` | `SessionTimeLeft` | 직접 대응 |
| 공기/트랙 온도 | `m_airTemperature`, `m_trackTemperature` | `AirTemp`, `TrackTemp`, 화씨 변형 | 직접/단위 변환 |
| 날씨 | `m_weather` | 직접 키 없음 | 공통 세션 상태로 파싱해 레이스 엔지니어의 우천·건조 전환 판단에 사용 |
| 예보/비 확률 | `m_weatherForecastSamples[]`, `m_rainPercentage` | 직접 키 없음 | 64개 예보 슬롯을 파싱해 강우 전략 콜과 리포트에 사용 |
| 마셜 구역 | `m_marshalZones[]` | `YellowFlag`, `GreenFlag` 등으로 파생 가능 | 차량 위치와 구역을 조합해야 정확함 |
| 차량 FIA 플래그 | `PacketCarStatusData.m_vehicleFiaFlags` | `YellowFlag`, `GreenFlag` | `-1=알 수 없음`, `0=없음`, `1=녹색`, `2=파란색`, `3=노란색` |
| 파랑/빨강/검정/흰색 플래그 | FIA 플래그, 이벤트, 결과 상태 조합 | MOZA F1 컬럼에서 일부 미지원 | 별도 검증 전에는 로컬 HUD/리포트용 |
| 세이프티카 | `m_safetyCarStatus`, `m_numSafetyCarPeriods`, `m_numVirtualSafetyCarPeriods` | 직접 키 없음 | 현재 SC/VSC 상태를 파싱해 레이스 컨트롤·재시작 콜과 전투 갭 억제에 사용 |
| 관전 중 | `m_isSpectating` | `Spectating`은 F1 24 컬럼 미지원 | 로컬 상태 표시 |

### 손상, 마모, 내구성

| 표시/분석 지표 | F1 25 원본 필드 | MOZA F1 키 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| 타이어 웨어 | `PacketCarDamageData.m_tyresWear[4]` | `TyreWearFL/FR/RL/RR` | 휠 인덱스 매핑 필요. 패킷 단위 순서 보정과 F1 24 호환 CarDamage 변환 구현됨 |
| 타이어 손상 | `m_tyresDamage[4]` | 직접 키 없음 | 로컬 HUD/리포트용 |
| 브레이크 손상 | `m_brakesDamage[4]` | 직접 키 없음 | 로컬 HUD/리포트용 |
| 타이어 블리스터 | `m_tyreBlisters[4]` | 직접 키 없음 | F1 25 신규/세부 손상 지표. 로컬 HUD/리포트용 |
| 앞 윙 손상 | `m_frontLeftWingDamage`, `m_frontRightWingDamage` | `WingWearFL`, `WingWearFR` | 직접 대응 |
| 뒤 윙 손상 | `m_rearWingDamage` | `WingWearR`는 F1 24 컬럼 미지원 | 로컬 HUD/리포트용 |
| 플로어/디퓨저/사이드팟 손상 | `m_floorDamage`, `m_diffuserDamage`, `m_sidepodDamage` | 직접 키 없음 | 로컬 손상 패널용 |
| 기어박스 손상 | `m_gearBoxDamage` | `GearBoxWear` | 직접 대응 |
| 엔진 손상 | `m_engineDamage` | `EngineWear` | 직접 대응 가능 |
| 엔진 부품 마모 | `m_engineMGUHWear`, `m_engineESWear`, `m_engineCEWear`, `m_engineICEWear`, `m_engineMGUKWear`, `m_engineTCWear` | 직접 키 없음 | 내구성 리포트용 |
| 엔진 파손/고착 | `m_engineBlown`, `m_engineSeized` | 직접 키 없음 | 알림용 |

### 세팅, 전략, 분석 전용 데이터

| 표시/분석 지표 | F1 25 원본 필드 | MOZA F1 키 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| 에어로 세팅 | `PacketCarSetupData.m_frontWing`, `m_rearWing` | 직접 키 없음 | 세팅 리포트, A/B 비교용 |
| 디퍼렌셜 | `m_onThrottle`, `m_offThrottle` | 직접 키 없음 | 트랙션/회전 추천 근거 |
| 서스펜션 지오메트리 | `m_frontCamber`, `m_rearCamber`, `m_frontToe`, `m_rearToe` | 직접 키 없음 | 타이어 온도/웨어 분석과 결합 |
| 서스펜션/ARB/차고 | `m_frontSuspension`, `m_rearSuspension`, `m_frontAntiRollBar`, `m_rearAntiRollBar`, `m_frontSuspensionHeight`, `m_rearSuspensionHeight` | 직접 키 없음 | 핸들링 추천 근거 |
| 브레이크 압력/바이어스 | `m_brakePressure`, `m_brakeBias` | `BrakeBias` 일부 대응 | 압력은 로컬 세팅 리포트용 |
| 세팅 타이어 압력 | `m_rearLeftTyrePressure`, `m_rearRightTyrePressure`, `m_frontLeftTyrePressure`, `m_frontRightTyrePressure` | 주행 중 압력 키와 구분 필요 | 세팅 값과 실시간 `m_tyresPressure[4]`를 분리해서 표시 |
| 타이어 세트 가용성 | `PacketTyreSetsData.m_tyreSetData[]`, `m_fittedIdx` | 직접 키 없음 | 전략/리포트용 |
| 랩/스틴트 히스토리 | `PacketSessionHistoryData.m_lapHistoryData[]`, `m_tyreStintsHistoryData[]` | 일부 랩 타임 키만 대응 | 스틴트 차트/리포트용 |
| 타임 트라이얼 비교 | `PacketTimeTrialData` 데이터셋 | 직접 키 없음 | 개인 최고/라이벌 비교용 |

### 모션, 맵, 물리

| 표시/분석 지표 | F1 25 원본 필드 | MOZA F1 키 가능성 | 브리지 처리 방향 |
| --- | --- | --- | --- |
| 월드 위치 | `PacketMotionData.m_carMotionData[playerCarIndex].m_worldPositionX/Y/Z` | `CarCoordinates01/02/03`, `Location` | 축 이름과 표시 정책 필요 |
| 속도 벡터 | `m_worldVelocityX/Y/Z`, `PacketMotionExData.m_localVelocityX/Y/Z` | `SpeedMs`와 일부 관련 | 로컬 물리/로깅용 |
| G-포스 | `m_gForceLateral`, `m_gForceLongitudinal`, `m_gForceVertical` | `GlobalAccelerationG`는 F1 24 컬럼 미지원 | 로컬 HUD/분석용 |
| 자세 | `m_yaw`, `m_pitch`, `m_roll` | `Heading`, `Pitch`, `Roll` | 라디안을 표시값으로 변환 |
| 앞바퀴 각도 | `PacketMotionExData.m_frontWheelsAngle` | 직접 키 없음 | 조향/언더스티어 분석용 |
| 휠 슬립/스핀 | `m_wheelSlipRatio[4]`, `m_wheelSlipAngle[4]`, `m_wheelSpeed[4]` | `WheelSpin` 파생 가능 | 트랙션/락업 분석용 |
| 휠 힘 | `m_wheelLatForce[4]`, `m_wheelLongForce[4]`, `m_wheelVertForce[4]` | 직접 키 없음 | 고급 분석용 |
| 에어로 높이/롤/섀시 | `m_frontAeroHeight`, `m_rearAeroHeight`, `m_frontRollAngle`, `m_rearRollAngle`, `m_chassisYaw`, `m_chassisPitch` | 일부 `Pitch/Roll`과 관련 | 세팅 분석용 |

## 현재 브리지 파서 범위

| F1 패킷 | 현재 코드 상태 | 빠진 주요 지표 |
| --- | --- | --- |
| `PacketSessionData` | 파싱: 총 랩 수, 트랙 길이, 세션 유형, 트랙 ID, 날씨·64개 예보, 트랙/공기 온도, 남은 세션 시간, 피트 제한 속도, SC/VSC, 마셜 구역, 피트 윈도·예상 복귀 순위 | 세션 설정 상세, 주말 구조, 보조 장치 설정 |
| `PacketLapData` | 파싱: 22/24대 전체 순위·피트/주행 상태, 랩 타임, 섹터 1/2 타임, 앞뒤/선두와의 차이, SC 델타, 랩 거리, 순위, 랩, 피트 횟수, 섹터, 무효 여부, 드라이버/결과 상태 | 페널티, 경고, 피트 타이머, 스피드 트랩 |
| `PacketCarTelemetryData` | 일부 파싱: 스로틀/브레이크/조향/클러치/속도/기어/RPM/DRS/REV/온도/압력 | 노면 유형, 추천 기어/MFD 패널 |
| `PacketCarStatusData` | 일부 파싱: 보조 장치, 브레이크 바이어스, 연료, RPM 제한, DRS, DRS 활성 거리, 피트 리미터, 타이어 컴파운드/사용 랩 수, ERS 저장·배포·MGU-K/H 회수량·2026 회수 한도 | FIA 플래그, 엔진 출력, 네트워크 일시정지 |
| `PacketFinalClassificationData` | 플레이어 최종 순위, 완주 랩, 그리드, 결과 상태·이유, 베스트 랩, 총 시간, 페널티, 타이어 스틴트 파싱 | 전체 차량 최종 분류 |
| `PacketCarDamageData` | 일부 파싱: 타이어 웨어/손상/블리스터, 윙 손상, 기어박스 손상, 엔진 손상 | 브레이크 손상, 플로어/디퓨저/사이드팟, 고장, 부품 마모 |
| `PacketCarSetupData` | 플레이어의 앞/뒤 윙, 온/오프 스로틀 디퍼렌셜, 캠버·토, 서스펜션·안티롤바·차고, 브레이크, 엔진 브레이킹, 세팅 타이어 압력, 밸러스트, 연료량 파싱 | 다른 차량 세팅은 게임이 공개하지 않음 |
| `PacketMotionData` | 미구현 | 위치, 속도 벡터, G-포스, 요/피치/롤 |
| `PacketParticipantsData` | 미구현 | 플레이어/팀/이름/차량 식별 정보 |
| `PacketEventData` | 미구현 | 페널티, 플래그, 스피드 트랩, 충돌, 추월 이벤트 |
| `PacketSessionHistoryData` | 미구현 | 베스트 랩/스틴트 히스토리 |
| `PacketTyreSetsData` | 플레이어의 20개 타이어 세트에 대해 실제/표시 컴파운드, 마모, 가용 여부, 권장 세션, 잔여·사용 가능 수명, 장착 상태 파싱 | 세트별 실제 온도 이력 |
| `PacketMotionExData` | 미구현 | 슬립/힘/에어로 높이/고급 물리 |

## 멀티플레이 텔레메트리 공개 범위

F1 25의 `Your Telemetry` 설정이 `Restricted`인 경우, 다른 플레이어 차량의 일부 값은 UDP에서 0으로 내려올 수 있습니다. 플레이어 본인 차량은 항상 볼 수 있지만, 상대 차량 분석이나 순위표 차이 외 세부 상태 분석에서는 이 제한을 고려해야 합니다.

제한 대상이 될 수 있는 대표 필드:

| 패킷 | `Restricted` 때 0 처리될 수 있는 값 |
| --- | --- |
| `PacketCarStatusData` | `m_fuelInTank`, `m_fuelCapacity`, `m_fuelMix`, `m_fuelRemainingLaps`, `m_frontBrakeBias`, `m_ersDeployMode`, `m_ersStoreEnergy`, `m_ersDeployedThisLap`, `m_ersHarvestedThisLapMGUK`, `m_ersHarvestedThisLapMGUH`, `m_enginePowerICE`, `m_enginePowerMGUK` |
| `PacketCarDamageData` | 윙/플로어/디퓨저/사이드팟 손상, `m_engineDamage`, `m_gearBoxDamage`, `m_tyresWear[4]`, `m_tyresDamage[4]`, `m_brakesDamage[4]`, DRS 고장, 엔진 부품 마모 |

## MOZA F1 계열 키와 F1 25 원본

표의 MOZA 키는 `Telemetry.get("v1/gameData/<MOZA key>").value`의 마지막 구간입니다.

| 영역 | MOZA 키 | F1 25 UDP 원본 | 비교 결과 |
| --- | --- | --- | --- |
| 레이스/랩 | `MaxRpm` | `PacketCarStatusData.m_maxRPM` | 직접 대응 |
| 레이스/랩 | `LapCount` | `PacketSessionData.m_totalLaps` | 직접 대응 |
| 레이스/랩 | `CarCount`, `OpponentCount` | `PacketParticipantsData.m_numActiveCars` | `OpponentCount`는 보통 `numActiveCars - 1` 파생 |
| 레이스/랩 | `Lap`, `CompletedLaps` | `PacketLapData.m_currentLapNum` | `CompletedLaps`는 현재 랩에서 파생 필요 |
| 레이스/랩 | `Pos` | `PacketLapData.m_carPosition` | 직접 대응 |
| 레이스/랩 | `LastLapTime`, `CurrentLapTime` | `PacketLapData.m_lastLapTimeInMS`, `m_currentLapTimeInMS` | 밀리초 값을 표시 형식으로 변환 |
| 레이스/랩 | `BestLapTime` | `PacketSessionHistoryData` 또는 `PacketFinalClassificationData.m_bestLapTimeInMS` | 직접/파생 대응 |
| 레이스/랩 | `PlayerIndex` | `PacketHeader.m_playerCarIndex` | 직접 대응 |
| 속도/입력 | `SpeedKmh`, `SpeedMph`, `SpeedMs` | `PacketCarTelemetryData.m_speed` | F1 원본은 km/h, mph와 m/s는 변환 |
| 속도/입력 | `Rpm` | `PacketCarTelemetryData.m_engineRPM` | 직접 대응 |
| 속도/입력 | `Gear` | `PacketCarTelemetryData.m_gear` | 직접 대응, `0=N`, `-1=R` 표시 변환 필요 |
| 속도/입력 | `Throttle`, `Brake` | `PacketCarTelemetryData.m_throttle`, `m_brake` | F1 원본은 `0.0..1.0`, MOZA 표시 방식에 따라 퍼센트 변환 가능 |
| 속도/입력 | `Clutch` | `PacketCarTelemetryData.m_clutch` | 직접 대응 |
| 속도/입력 | `Drs` | `PacketCarTelemetryData.m_drs` | 직접 대응 |
| 속도/입력 | `CarSettings_CurrentDisplayedRPMPercent` | `PacketCarTelemetryData.m_revLightsPercent` 또는 `m_engineRPM / m_maxRPM` | Pit House가 어느 쪽을 쓰는지 확인 필요 |
| 보조/상태 | `ABSLevel` | `PacketCarStatusData.m_antiLockBrakes` | F1은 `0/1`; 레벨 표현은 라벨 변환 |
| 보조/상태 | `TCLevel` | `PacketCarStatusData.m_tractionControl` | F1은 `0/1/2`; 레벨 표현 가능 |
| 보조/상태 | `BrakeBias` | `PacketCarStatusData.m_frontBrakeBias` | 직접 대응 |
| 보조/상태 | `DRSAllowed` | `PacketCarStatusData.m_drsAllowed` | 직접 대응 |
| 보조/상태 | `DRSAvailable` | `PacketCarStatusData.m_drsActivationDistance` | `> 0`이면 사용 가능으로 파생 |
| 보조/상태 | `PitLimiter` | `PacketCarStatusData.m_pitLimiterStatus` | 직접 대응 |
| 보조/상태 | `IsInPit`, `Pitlane` | `PacketLapData.m_pitStatus` | `1=피팅 중`, `2=피트 구역 안` 열거값 변환 |
| 보조/상태 | `EngineIgnition`, `EngineStarted` | 직접 필드 없음 | 보통 `m_engineRPM > 0` 또는 세션 상태에서 파생 |
| 보조/상태 | `CarSettings_MaxGears` | `PacketCarStatusData.m_maxGears` | 직접 대응 |
| 연료/에너지 | `FuelRemain`, `Fuel` | `PacketCarStatusData.m_fuelInTank` | F1 원본은 연료 질량 |
| 연료/에너지 | `FuelRemainLaps`, `FuelSurplusLaps` | `PacketCarStatusData.m_fuelRemainingLaps` | 직접/파생 대응 |
| 연료/에너지 | `FuelClass` | `m_fuelRemainingLaps` 또는 연료 차이 | 라벨/색상 파생 |
| 연료/에너지 | `FuelCapacity` | `PacketCarStatusData.m_fuelCapacity` | 직접 대응 |
| 연료/에너지 | `EnergyRemain`, `Ers`, `ERSPercent`, `ERSStored` | `PacketCarStatusData.m_ersStoreEnergy` | F1 원본은 줄, 퍼센트는 보통 `4,000,000J` 기준 파생 |
| 연료/에너지 | `ERSMax` | F1 고정 기준값 또는 게임 규칙 | 보통 `4,000,000J` 상수 |
| 연료/에너지 | `EnergyDeployed` | `PacketCarStatusData.m_ersDeployedThisLap` | 직접 대응 |
| 연료/에너지 | `EnergyHarvested` | `m_ersHarvestedThisLapMGUK + m_ersHarvestedThisLapMGUH` | 합산 파생 |
| 타이어/브레이크 | `TyreTempFL`, `TyreTempFR`, `TyreTempRL`, `TyreTempRR` | `PacketCarTelemetryData.m_tyresSurfaceTemperature[4]` | F1 휠 순서 `RL, RR, FL, FR`를 키 이름으로 매핑 필요 |
| 타이어/브레이크 | `TyreTempFLI`, `TyreTempFRI`, `TyreTempRLI`, `TyreTempRRI` | `PacketCarTelemetryData.m_tyresInnerTemperature[4]` | 휠 순서 매핑 필요 |
| 타이어/브레이크 | `TyreTempFL&F`, `TyreTempFLI&F`, `TyreTempFR&F`, `TyreTempFRI&F`, `TyreTempRL&F`, `TyreTempRLI&F`, `TyreTempRR&F`, `TyreTempRRI&F` | 표면/내부 타이어 온도 | 화씨 표시 키로 보면 단위 변환 |
| 타이어/브레이크 | `TyrePressureFL`, `TyrePressureFR`, `TyrePressureRL`, `TyrePressureRR` | `PacketCarTelemetryData.m_tyresPressure[4]` | F1 원본은 PSI, 휠 순서 매핑 필요 |
| 타이어/브레이크 | `BrakeTempFL`, `BrakeTempFR`, `BrakeTempRL`, `BrakeTempRR` | `PacketCarTelemetryData.m_brakesTemperature[4]` | 섭씨, 휠 순서 매핑 필요 |
| 타이어/브레이크 | `BrakeTempFL&F`, `BrakeTempFR&F`, `BrakeTempRL&F`, `BrakeTempRR&F` | 브레이크 온도 | 화씨 표시 키로 보면 단위 변환 |
| 타이어/브레이크 | `TrackTemp`, `AirTemp` | `PacketSessionData.m_trackTemperature`, `m_airTemperature` | 직접 대응 |
| 타이어/브레이크 | `TrackTemp&F`, `AirTemp&F` | 트랙/공기 온도 | 화씨 표시 키로 보면 단위 변환 |
| 손상/마모 | `TyreWearFL`, `TyreWearFR`, `TyreWearRL`, `TyreWearRR` | `PacketCarDamageData.m_tyresWear[4]` | 휠 순서 보정 필요. 패킷 단위 순서 보정과 F1 24 호환 CarDamage 변환 구현됨 |
| 손상/마모 | `WingWearFL`, `WingWearFR` | `PacketCarDamageData.m_frontLeftWingDamage`, `m_frontRightWingDamage` | 직접 대응 |
| 손상/마모 | `EngineWear` | `PacketCarDamageData.m_engineDamage` 또는 부품 마모 필드 | 단일 값 선택 정책 필요 |
| 손상/마모 | `GearBoxWear` | `PacketCarDamageData.m_gearBoxDamage` | 직접 대응 |
| 플래그/세션 | `YellowFlag`, `GreenFlag` | `PacketCarStatusData.m_vehicleFiaFlags`, `PacketSessionData.m_marshalZones[]` | 열거값/구역 해석 필요 |
| 플래그/세션 | `SectorIndex` | `PacketLapData.m_sector` | 직접 대응 |
| 플래그/세션 | `Sector1Time`, `Sector2Time` | `PacketLapData.m_sector1Time*`, `m_sector2Time*` | 분/밀리초 조합 변환 |
| 플래그/세션 | `SessionTimeLeft` | `PacketSessionData.m_sessionTimeLeft` | 직접 대응 |
| 플래그/세션 | `LapInvalidated` | `PacketLapData.m_currentLapInvalid` | 직접 대응 |
| 플래그/세션 | `SessionTypeName` | `PacketSessionData.m_sessionType` | 열거값 라벨 변환 |
| 플래그/세션 | `TrackId` | `PacketSessionData.m_trackId` | 직접 대응 |
| 플래그/세션 | `MapName` | `PacketSessionData.m_trackId` | 트랙 ID를 이름 라벨로 변환 |
| 플래그/세션 | `TrackLength` | `PacketSessionData.m_trackLength` | 직접 대응 |
| 모션/맵 | `Heading` | `PacketMotionData.m_yaw` | 라디안을 표시 헤딩으로 변환 |
| 모션/맵 | `Pitch`, `Roll` | `PacketMotionData.m_pitch`, `m_roll` 또는 `PacketMotionExData.m_chassisPitch` | 직접/파생 대응 |
| 모션/맵 | `CarCoordinates01`, `CarCoordinates02`, `CarCoordinates03`, `Location` | `PacketMotionData.m_worldPositionX/Y/Z` | 이름별 축 매핑 필요 |
| 모션/맵 | `TrackPositionPercent` | `PacketLapData.m_lapDistance / PacketSessionData.m_trackLength` | 파생 |
| 모션/맵 | `WheelSpin` | `PacketMotionExData.m_wheelSlipRatio[4]`, `m_wheelSpeed[4]` | 파생 |
| 식별 정보 | `CarId`, `CarModel` | `PacketParticipantsData.m_teamId`, `m_techLevel`, `m_carNumber` | Pit House 표시 정책 필요 |
| 식별 정보 | `PlayerName` | `PacketParticipantsData.m_name` | 온라인 이름 설정에 영향 |
| 식별 정보 | `Gamename` | 게임/프로필 상수 | F1 패킷 필드라기보다 어댑터 라벨 |

## MOZA 전역 키지만 F1 24 컬럼에 표시가 없는 키

아래 키는 MOZA 전체 표에는 있지만 `F1 24` 컬럼에는 지원 표시가 없습니다. F1 25 UDP에 비슷한 원본이 있더라도 Pit House F1 대시에서 그대로 쓸 수 있다고 보면 안 됩니다.

| MOZA 키 | F1 25에 비슷한 원본이 있는가 | 비고 |
| --- | --- | --- |
| `Gap`, `EstimatedLapTime` | 있음 | `m_deltaToCarInFront*`, `m_deltaToRaceLeader*`, 랩 페이스로 파생 가능하지만 F1 컬럼 미지원 |
| `ABS`, `TC` | 있음 | `ABSLevel`, `TCLevel`은 지원 표시가 있으나 단순 불리언 키는 F1 컬럼 미지원 |
| `ECUMap`, `Boost` | 제한적/없음 | F1 25 현대 F1 ERS/엔진 모델과 직접 1:1 아님 |
| `TyreTempFLM`, `TyreTempFRM`, `TyreTempRLM`, `TyreTempRRM` | 없음 | F1은 surface/inner만 제공 |
| `TyreTempFLO&F`, `TyreTempFRO&F`, `TyreTempRLO&F`, `TyreTempRRO&F`, `TyreTempFLO`, `TyreTempFRO`, `TyreTempRL0`, `TyreTempRRO` | 없음 | 외부/중간 계열 키로 보이며 F1 25 UDP에는 직접 값 없음. `TyreTempRL0`는 MOZA 원문 표기 유지 |
| `FuelTemp`, `WaterTemperature`, `OilPressure` | 없음 | F1 25 UDP에는 직접 대응 필드 없음 |
| `WingWearR` | 있음 | `m_rearWingDamage`가 있지만 MOZA F1 컬럼에는 지원 표시 없음 |
| `ReverseLight` | 파생 가능 | `m_gear == -1`로 파생 가능하지만 F1 컬럼 미지원 |
| `BlueFlag`, `WhiteFlag`, `RedFlag`, `Flag_Black` | 부분적으로 있음 | FIA 플래그, 마셜 구역, 이벤트/결과 상태에서 추론 가능하지만 키 지원 표시 없음 |
| `AccX`, `AccY`, `AccZ`, `GlobalAccelerationG` | 부분적으로 있음 | F1 모션 패킷에 G-포스와 속도 벡터가 있으나 키 지원 표시 없음 |
| `SectorsCount` | 파생 가능 | F1은 일반적으로 3섹터. 키 지원 표시 없음 |
| `Spectating` | 있음 | `m_isSpectating`이 있지만 키 지원 표시 없음 |
| `ReplayMode`, `Ontrack` | 명확한 직접 필드 없음 | 다른 게임 어댑터용 키로 보는 것이 안전 |

## F1 25에는 있지만 MOZA 키로 보존하기 어려운 데이터

F1 25 UDP의 원본 필드는 MOZA 키보다 더 넓습니다. 다음 정보는 F1 패킷에는 있지만 Pit House Digital Dash 키와 직접 대응하지 않습니다.

| F1 패킷 | MOZA 키 대응이 약한 값 |
| --- | --- |
| `PacketCarSetupData` | 앞/뒤 윙, 디퍼렌셜, 캠버, 토, 서스펜션, 안티롤바, 차고, 브레이크 압력, 엔진 브레이킹, 세팅 타이어 압력, 밸러스트, 연료량 |
| `PacketSessionData` | 날씨 예보 샘플, 마셜 구역 목록, 보조 장치 설정, 규칙 세트, 세이프티카/레드 플래그 횟수, 주말 구조, 레이스 설정 |
| `PacketParticipantsData`, `PacketLobbyInfoData` | AI 제어 여부, 드라이버/팀/국적/플랫폼 메타데이터, 텔레메트리 공개 플래그, 리버리 색상, 로비 준비 상태 |
| `PacketEventData` | 패스티스트 랩 이벤트, 리타이어 사유, 페널티 상세, 스피드 트랩 이벤트, 플래시백, 버튼 플래그, 충돌/추월/세이프티카 이벤트 |
| `PacketFinalClassificationData` | 포인트, 페널티, 타이어 스틴트 히스토리, 결과 사유, 총 레이스 시간 상세 |
| `PacketSessionHistoryData` | 일부 대시보드 랩 타임 키를 넘어서는 모든 랩/섹터 히스토리와 스틴트 히스토리 |
| `PacketTyreSetsData` | 사용 가능한 타이어 세트, 권장 세션, 수명, 사용 가능 수명, 장착 인덱스 |
| `PacketMotionExData` | 서스펜션 위치/속도/가속도, 휠 슬립 각도, 휠 힘, 에어로 높이, 롤 각도, 섀시 요/피치, 캠버, 캠버 게인 |
| `PacketLapPositionsData` | 랩별 전체 순위 차트 데이터 |
| `PacketTimeTrialData` | 타임 트라이얼 비교용 개인 최고/라이벌 데이터셋과 보조 장치 |

## 브리지 구현 우선순위

1. `m_tyresWear[4]` 패킷 재매핑은 선택 보정으로만 유지합니다.
2. 로컬 전용 HUD 필드를 새로 만들기 전에, MOZA F1 계열 키가 이미 있는 값의 파서 범위를 먼저 늘립니다.
3. Pit House가 일치하는 키를 노출하지 않는 한 차간/델타, 세팅, 예보, 이벤트, 타이어 세트 데이터는 로컬 HUD/리포트 기능으로 취급합니다.
4. 모든 휠 배열 키는 표시 또는 재매핑 전에 F1 순서 `RL, RR, FL, FR`에서 이름 기반 코너로 정규화합니다.
