# MOZA 대시보드 바인딩 메모

MOZA Dash Studio는 텍스트, 게이지, 색상, 애니메이션 표현식을 텔레메트리 값에 연결할 수 있습니다.

```js
Telemetry.get("v1/gameData/Rpm").value
```

접두어 의미:

- `v1`: 바인딩 API 네임스페이스
- `gameData`: Pit House가 지원 게임에서 채우는 텔레메트리 네임스페이스
- 마지막 구간: Digital Dash 지원 목록에 표시되는 MOZA 텔레메트리 이름

## F1 25 대시보드에서의 의미

F1 25는 원본 바이너리 UDP 패킷을 보냅니다. Pit House는 이 패킷을 파싱하고 선택된 값을 `v1/gameData/...` 키로 게시합니다. UDP 브리지는 Pit House가 받기 전에 패킷 값을 바꿀 수 있지만, 임의의 대시보드 키를 새로 추가할 수는 없습니다.

따라서:

- F1 25 tyre wear는 `m_carDamageData[playerCarIndex].m_tyresWear[4]`이며 순서는 `0=RL`, `1=RR`, `2=FL`, `3=FR`입니다.
- Pit House가 차량 손상 패킷에서 타이어 웨어 값을 읽는다면, `m_tyresWear[4]` 변경은 `v1/gameData/TyreWearFL` 계열 키에 영향을 줄 수 있습니다.
- `TyreWear*` 값이 계속 `100`이면 순서 보정보다 F1 25 CarDamage 레이아웃 호환 문제일 가능성이 큽니다. 현재 브리지는 CarDamage 패킷만 F1 24 호환 레이아웃으로 자동 변환합니다.
- UDP 패킷 전달만으로 새 `v1/gameData/BehindGap` 키를 만드는 것은 기대하면 안 됩니다.
- 뒤차와의 차이는 Pit House가 이미 노출하는 기존 필드에 억지로 넣거나, 이 브리지를 직접 읽는 별도 대시보드에서 렌더링해야 합니다.

전체 원본 필드와 인덱스 매핑은 [F1 25 / MOZA 텔레메트리 매트릭스](f1-25-moza-telemetry-matrix.md)를 봅니다.

## 표현식 예시

DRS 배지:

```js
Telemetry.get("v1/gameData/DRSAvailable").value ? "DRS" : ""
```

ERS 색상:

```js
Telemetry.get("v1/gameData/ERSPercent").value < 20 ? "#FF3B30" : "#21D17C"
```

RPM 경고:

```js
Telemetry.get("v1/gameData/CarSettings_CurrentDisplayedRPMPercent").value > 95 ? "#FF3B30" : "#E8EDF2"
```

연료 잔여 랩 소수점 한 자리:

```js
Telemetry.get("v1/gameData/FuelRemainLaps").value.toFixed(1)
```
