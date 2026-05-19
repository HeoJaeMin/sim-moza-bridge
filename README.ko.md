# Sim MOZA Bridge

[English README](README.md)

시뮬레이싱 게임 텔레메트리를 받아 MOZA Pit House로 넘기고, 일부 패킷 보정, 로깅, HUD, 분석을 실험하는 Rust 기반 브리지입니다. 현재 가장 잘 지원되는 대상은 F1 25 UDP 텔레메트리입니다.

이 프로젝트는 MOZA 또는 EA의 공식 도구가 아닙니다.

## 지원 프로필

| 게임 | 프로필 | 현재 지원 상태 | 비고 |
| --- | --- | --- | --- |
| 자동 감지 | `auto` | UDP 패킷 기반 감지 | 기본값. 현재 F1 25를 감지하고, 알 수 없는 UDP는 그대로 전달합니다 |
| F1 25 | `f1-25` | UDP passthrough + F1 패킷 보정 | 명시적으로 F1 25만 사용할 때 선택합니다 |
| Generic UDP | `generic-udp` | UDP passthrough | 외부 도구가 이미 MOZA가 이해할 수 있는 UDP를 만들어줄 때 사용합니다 |
| Assetto Corsa EVO | `ace` | 문서화만 완료, adapter 미구현 | 공식 업데이트와 공개 integration 모두 단순 UDP가 아니라 shared memory 계열을 가리킵니다 |
| Assetto Corsa Rally | `acr` | 문서화만 완료, adapter 미구현 | MOZA는 telemetry 지원을 표시하지만, 이 브리지에는 native/helper reader가 필요합니다 |
| Le Mans Ultimate | `lmu` / `lu` | 문서화만 완료, adapter 미구현 | MOZA telemetry 지원과 Digital Dash의 LMU key 컬럼이 확인됩니다 |

자동 감지는 실행 중인 프로세스 이름이 아니라 들어오는 텔레메트리 패킷을 기준으로 합니다. 그래서 현재는 F1 25 UDP 패킷만 안정적으로 판별할 수 있습니다. ACE, ACR, LMU는 native shared-memory adapter 또는 외부 UDP exporter가 생기기 전까지 자동 판별할 수 없습니다.

ACE, ACR, LU/LMU 조사 내용은 [docs/game-adapter-research.md](docs/game-adapter-research.md)에 정리했습니다.

## 요구 사항

- 대상 게임과 MOZA Pit House가 실행되는 Windows PC
- 소스에서 직접 빌드할 경우 Rust 1.95 이상
- UDP 기반 프로필의 경우 게임 내 UDP 텔레메트리 활성화

집 PC에서 일반 실행만 할 때는 `sim-moza-bridge.exe`만 있으면 됩니다. Rust toolchain은 개발하거나 다시 빌드할 때만 필요합니다.

## F1 25 설정

F1 25의 Telemetry Settings에서 다음처럼 설정합니다.

| 항목 | 값 |
| --- | --- |
| UDP Telemetry | On |
| UDP IP Address | `127.0.0.1` |
| UDP Port | `20777` |
| UDP Send Rate | HUD 부드러움 기준 `60Hz` 권장, 안정성 우선이면 `20Hz`, `120Hz`는 게임이 허용할 때 실험용 |
| UDP Format | `2025` |

MOZA Pit House는 F1 25 텔레메트리 입력으로 보통 `22025` 포트를 기대하므로, 브리지는 기본적으로 `20777`에서 받아 `22025`로 전달합니다.

## 사용법

기본 passthrough:

```bash
cargo run -- --listen 20777 --moza-port 22025 --mode passthrough
```

배포된 exe로 실행:

```powershell
.\sim-moza-bridge.exe --listen 20777 --moza-port 22025 --mode passthrough
```

F1 25 타이어 웨어 순서 보정:

```bash
cargo run -- --listen 20777 --moza-port 22025 --mode remap --fix-tyre-wear-order
```

verbose 출력:

```bash
cargo run -- --mode remap --fix-tyre-wear-order --verbose
```

dry-run:

```bash
cargo run -- --mode remap --fix-tyre-wear-order --dry-run
```

외부 exporter가 만든 UDP를 그대로 전달:

```bash
cargo run -- --game generic-udp --listen 20777 --moza-port 22025 --mode passthrough
```

## 입력 로깅, HUD, 분석

F1 25의 `PacketCarTelemetryData`에서 throttle, brake, steer, clutch, DRS, REV, speed, gear, RPM, 온도 값을 추출합니다.

CSV 로깅:

```bash
cargo run -- --mode remap --fix-tyre-wear-order --input-log inputs.csv
```

브라우저 HUD는 throttle/brake/steer 바, REV LED, DRS 상태, 입력 trace를 표시합니다.

```bash
cargo run -- --hud-http 8765 --input-log inputs.csv
```

브라우저에서 엽니다.

```text
http://127.0.0.1:8765
```

HUD는 약 60Hz 기준으로 화면을 갱신합니다. 사람이 보는 throttle/brake 바는 60Hz면 충분하고, 120Hz 이상은 화면 표시보다 고주파 로깅이나 분석 쪽에 더 의미가 있습니다.

랩 분석은 완료된 랩을 20개 거리 segment로 나눠 CSV와 Markdown 리포트를 만듭니다.

```bash
cargo run -- \
  --mode remap \
  --fix-tyre-wear-order \
  --input-log inputs.csv \
  --corner-log corners.csv \
  --analysis-report analysis.md
```

`--corner-log`는 완료된 랩의 segment별 속도, 브레이크, 스로틀, 조향 요약을 CSV로 누적합니다. `--analysis-report`는 랩이 끝날 때마다 최신 Markdown 리포트를 덮어씁니다. 리포트에는 clean lap 여부, 타이어 웨어, 현재 연료/브레이크 바이어스/ERS 상태, 세팅 후보가 들어갑니다.

세팅 추천은 자동 정답이 아니라 후보입니다. 예를 들어 mid-corner 조향량과 앞 타이어 웨어/온도가 높으면 front grip 후보를, corner exit에서 스로틀과 조향 보정이 같이 커지면 rear traction 후보를 제안합니다. 같은 연료량, 같은 타이어 age에서 A/B 테스트로 확인해야 합니다.

## 텔레메트리 호환 차이

F1 25 UDP는 바이너리 프로토콜입니다. MOZA Dash Studio는 `v1/gameData/Rpm`, `v1/gameData/TyreWearFL` 같은 이름 기반 값을 노출합니다. 두 형식이 항상 1:1로 맞지는 않습니다.

전체 field 비교표는 [docs/f1-25-moza-telemetry-matrix.md](docs/f1-25-moza-telemetry-matrix.md)에 정리했습니다.

알려진 차이 범주는 다음과 같습니다.

| 영역 | 왜 문제가 되는가 | 현재 브리지 동작 |
| --- | --- | --- |
| 휠 배열 | F1 휠 배열은 `RL, RR, FL, FR` 순서이고, 대시보드 key는 보통 `FL, FR, RL, RR` 이름 기준입니다. 타이어 웨어뿐 아니라 타이어 데미지, 타이어 온도, 타이어 압력, 브레이크 온도에도 영향을 줄 수 있습니다. | 내부 로깅/HUD 파서는 F1 휠 배열을 이름 기준 corner로 매핑합니다. 패킷 forwarding에서 실제로 고치는 값은 현재 `--fix-tyre-wear-order`의 타이어 웨어뿐입니다. |
| 단위와 파생값 | F1 패킷은 ERS 저장 에너지, fuel in tank, fuel remaining laps, rev-light percent, 온도처럼 raw/game-specific 값을 냅니다. MOZA key는 percent, laps, label, normalized value일 수 있습니다. | 로컬 HUD/report에서는 표시용 값을 일부 파생합니다. forwarding 패킷은 명시적인 remap 기능 외에는 전체 단위 변환을 하지 않습니다. |
| 상태와 enum | DRS, ERS deploy mode, 타이어 compound, pit status, invalid lap, result status는 게임 enum입니다. 대시보드는 boolean, label, color를 기대하는 경우가 많습니다. | 구현된 범위에서는 로컬 HUD/분석용으로 파싱합니다. MOZA 대시 동작은 Pit House가 이미 제공하는 key에 의존합니다. |
| 랩과 gap 데이터 | F1에는 lap distance, lap number, invalid flag, car position, delta-to-front, delta-to-leader가 있습니다. MOZA가 `BehindGap`, `FrontGap` 같은 대응 key를 제공하지 않을 수 있습니다. | 로컬 분석 리포트에서 사용합니다. 브리지가 Pit House 안에 새 MOZA telemetry key를 만들 수는 없습니다. |
| 패킷 버전 차이 | F1 24와 F1 25는 packet id가 비슷해도 layout이 다릅니다. | 분석 파싱은 F1 25 format `2025`일 때만 수행합니다. 미지원 format은 passthrough될 수 있지만 로컬 분석에는 쓰지 않습니다. |
| 비-F1 게임 | ACE, ACR, LMU는 F1 UDP와 같은 packet shape가 아닙니다. shared-memory/plugin adapter 또는 외부 UDP exporter가 필요합니다. | 프로필은 등록되어 있지만 native adapter는 아직 미구현입니다. `generic-udp`는 외부 exporter가 만든 패킷만 그대로 전달합니다. |

현재 구현된 packet-level remap은 타이어 웨어 순서 보정입니다. F1 25의 휠 배열은 다음 순서를 씁니다.

```text
0 = RL
1 = RR
2 = FL
3 = FR
```

반면 MOZA 대시보드 필드는 이름 기준으로 노출됩니다.

```text
TyreWearFL
TyreWearFR
TyreWearRL
TyreWearRR
```

MOZA Pit House가 F1 25 배열을 이미 올바르게 매핑한다면 `--fix-tyre-wear-order`를 켜지 않는 것이 맞습니다. 실제 Mission R 대시에 타이어 웨어가 앞뒤/좌우로 바뀌어 보일 때만 이 옵션을 켜고 확인해야 합니다.

## MOZA Dash Studio 바인딩

MOZA Dash Studio는 JavaScript 표현식으로 텔레메트리를 읽습니다.

```js
Telemetry.get("v1/gameData/Rpm").value
```

대부분의 대시보드 값은 다음 형태입니다.

```text
v1/gameData/<TelemetryName>
```

예시:

| 표시 | 바인딩 |
| --- | --- |
| Gear | `Telemetry.get("v1/gameData/Gear").value` |
| RPM | `Telemetry.get("v1/gameData/Rpm").value` |
| Speed | `Telemetry.get("v1/gameData/SpeedKmh").value` |
| DRS | `Telemetry.get("v1/gameData/Drs").value` |
| ERS | `Telemetry.get("v1/gameData/ERSPercent").value` |
| Fuel laps | `Telemetry.get("v1/gameData/FuelRemainLaps").value` |
| Brake bias | `Telemetry.get("v1/gameData/BrakeBias").value` |
| Front-left tyre wear | `Telemetry.get("v1/gameData/TyreWearFL").value` |

이 브리지는 새 MOZA key를 등록하지 않습니다. 예를 들어 Pit House가 `v1/gameData/BehindGap`을 제공하지 않는다면, 브리지만으로 그 key를 새로 만들 수는 없습니다. 브리지는 Pit House가 읽는 기존 게임 패킷 값을 바꾸거나 전달할 수 있습니다.

## 옵션

| 옵션 | 기본값 | 설명 |
| --- | --- | --- |
| `--game` | `auto` | `auto`, `f1-25`, `generic-udp`, `ace`, `acr`, `lmu` |
| `--listen` | 프로필 기본값 | 게임 UDP를 받는 포트 |
| `--listen-host` | `127.0.0.1` | 수신 host/interface. 다른 PC에서 LAN으로 보낼 때만 `0.0.0.0` 사용 |
| `--moza-host` | `127.0.0.1` | MOZA Pit House host |
| `--moza-port` | 프로필 기본값 | MOZA Pit House 대상 포트 |
| `--mode` | `passthrough` | `passthrough` 또는 `remap` |
| `--fix-tyre-wear-order` | `false` | F1 25 `m_tyresWear[4]` 순서 보정 |
| `--input-log` | 없음 | throttle/brake/steer/speed/gear/RPM/온도 CSV 저장 경로 |
| `--corner-log` | 없음 | 완료된 랩의 segment 요약 CSV 저장 경로 |
| `--analysis-report` | 없음 | 최신 완료 랩 분석 Markdown 저장 경로 |
| `--hud-http` | 없음 | 지정한 포트로 로컬 HTTP HUD 실행 |
| `--hud-host` | `127.0.0.1` | 로컬 HTTP HUD host/interface |
| `--dry-run` | `false` | 패킷을 MOZA로 전달하지 않음 |
| `--verbose` | `false` | 런타임 통계 출력 |

## 현재 범위

구현됨:

- F1 25 packet header parsing
- F1 25 UDP packet 기반 `auto` 감지
- MOZA Pit House로 UDP passthrough
- `PacketCarDamageData` 타이어 웨어 순서 보정
- `PacketCarTelemetryData`에서 throttle/brake/steer/clutch/DRS/REV/speed/gear/RPM/온도 추출
- 분석용 player lap/session/car status/car damage parsing
- `--input-log` CSV 로깅
- `--corner-log` 완료 랩 segment CSV 로깅
- `--analysis-report` clean lap 판정과 세팅 후보 Markdown 리포트
- REV LED, steering bar, input trace가 포함된 `--hud-http` 브라우저 HUD
- ACE/ACR/LMU placeholder 프로필과 명확한 에러 메시지
- Rust unit test

아직 미구현:

- ACE shared-memory adapter
- ACR shared-memory/helper adapter
- LMU shared-memory/plugin adapter
- MOZA 대시에 behind gap 새 필드 주입
- F1 25 -> F1 24 packet down-conversion
- SimHub 호환 대시보드 에디터
- Mission R OLED 직접 렌더링
- 서명된 Windows installer/release pipeline

## 안전 메모

이 브리지는 F1 25로 입력을 되돌려 보내지 않습니다. UDP 텔레메트리를 읽고, 필요 시 MOZA Pit House로 전달합니다.

텔레메트리가 멈추면 다음 순서로 다시 시작합니다.

1. MOZA Pit House
2. Sim MOZA Bridge
3. 대상 게임
