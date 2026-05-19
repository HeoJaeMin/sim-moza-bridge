# HUD와 입력 로깅

Rust 브리지는 F1 25의 `PacketCarTelemetryData`에서 플레이어 차량 입력 값을 추출하고, 랩/세션/상태/손상 패킷과 결합해 로컬 분석에 사용할 수 있습니다.

현재 추출하는 주요 값:

- `session_time`
- `frame_identifier`
- `player_car_index`
- `throttle`
- `brake`
- `steer`
- `clutch`
- `speed_kmh`
- `gear`
- `rpm`
- `drs`
- `rev_lights_percent`
- 브레이크 온도
- 타이어 온도
- 타이어 압력

## CSV 로깅

```bash
cargo run -- --input-log inputs.csv
```

CSV 행은 파일 끝에 추가됩니다. 파일이 없거나 비어 있으면 헤더를 먼저 기록합니다.

## 브라우저 HUD

```bash
cargo run -- --hud-http 8765
```

브라우저에서 엽니다.

```text
http://127.0.0.1:8765
```

HUD는 `/state`를 약 60Hz로 폴링하고 스로틀/브레이크/조향 바, 속도, 기어, RPM, DRS, REV LED, 프레임, 흐르는 입력 추적 그래프를 표시합니다.

## 랩 분석

```bash
cargo run -- --corner-log corners.csv --analysis-report analysis.md
```

`--corner-log`는 완료된 랩의 구간 요약을 기록합니다. F1 세션의 트랙 길이를 사용할 수 있으면 한 랩을 거리 기준 20개 버킷으로 나눕니다.

`--analysis-report`는 최신 완료 랩의 Markdown 스냅샷을 덮어씁니다. 포함 내용:

- 클린 랩 여부
- 랩 타임과 샘플 수
- `PacketCarStatusData`가 있을 때 현재 연료, 브레이크 바이어스, ERS, 타이어 사용 랩 수
- `PacketCarDamageData`가 있을 때 타이어 웨어
- 구간 추적 표
- 입력 추적, 타이어 웨어, 타이어 온도, 상태 휴리스틱 기반 세팅 후보

## 범위

이 기능은 완전한 SimHub 복제본이 아닙니다. 로컬 HUD와 분석 경로는 빠른 운전자 피드백, 반복 가능한 CSV 출력, Markdown 리포트를 위한 기능입니다.

후속 후보:

- 폴링 대신 WebSocket 또는 Server-Sent Events 전송 추가
- 재사용 가능한 대시보드 레이아웃 파일 추가
- 네이티브 ACE, ACR, LMU 입력 어댑터 추가
