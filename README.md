# Sim MOZA Bridge

시뮬레이싱 UDP 텔레메트리를 받아 MOZA Pit House로 넘기는 Rust 기반 브리지입니다.
일반 실행은 자동 감지로 시작하고, F1 25가 감지되면 MOZA Pit House 정합성에 맞는 보정을 자동 적용합니다.

이 프로젝트는 MOZA 또는 EA의 공식 도구가 아닙니다.

HTML 문서는 [README.html](README.html)에서 볼 수 있습니다.

## 요구 사항

- 대상 게임과 MOZA Pit House가 실행되는 Windows PC
- 소스에서 직접 빌드할 경우 Rust 1.95 이상
- F1 25 게임 내 UDP 텔레메트리 활성화

집 PC에서 일반 실행만 할 때는 `sim-moza-bridge.exe`만 있으면 됩니다. Rust toolchain은 개발하거나 다시 빌드할 때만 필요합니다.

## F1 25 설정

F1 25의 텔레메트리 설정에서 다음처럼 설정합니다.

| 항목 | 값 |
| --- | --- |
| UDP Telemetry | On |
| UDP IP Address | `127.0.0.1` |
| UDP Port | `20777` |
| UDP Send Rate | HUD 부드러움 기준 `60Hz` 권장, 안정성 우선이면 `20Hz` |
| UDP Format | `2025` |

MOZA Pit House는 F1 25 텔레메트리 입력으로 보통 `22025` 포트를 기대하므로, 브리지는 기본적으로 `20777`에서 받아 `22025`로 전달합니다.

## LMU / ACE 설정

LMU와 ACE는 F1 25처럼 게임 안 UDP 포트만 맞추는 방식으로 이 브리지에 직접 연결되지 않습니다. Windows 게임 PC에서 각 게임을 실행한 뒤 해당 adapter를 선택합니다.

```bash
cargo run -- --game lmu
cargo run -- --game ace
```

- LMU adapter는 `LMU_Data` 공유 메모리를 읽고 HUD에 속도, RPM, 기어, 입력, 타이어/브레이크, 연료, 앞/뒤 차 gap을 표시합니다.
- ACE adapter는 `Local\acevo_pmf_physics` 공유 메모리를 읽고 HUD에 기본 주행 텔레메트리를 표시합니다.
- 공유 메모리가 아직 없으면 Windows에서는 adapter가 켜진 상태로 게임 세션을 기다립니다.

## 실행

기본 실행:

```bash
cargo run
```

배포된 실행 파일:

```powershell
.\sim-moza-bridge.exe
```

포트를 바꿔야 할 때만 지정합니다.

```bash
cargo run -- --listen 20777 --moza-port 22025
```

런타임 통계와 패치 로그를 보고 싶을 때만 debug를 켭니다.

```bash
cargo run -- --debug
```

브리지가 시작되면 로컬 HUD가 기본 브라우저로 자동 열립니다.

```text
http://127.0.0.1:8765
```

HUD에는 속도, 기어, RPM, REV LED, 입력 추적, 타이어/브레이크 상태, 앞차/뒤차/선두 gap, 연료, ERS, 손상 패널이 기본으로 표시됩니다.

## 동작 방식

브리지는 자동 감지 모드로 동작합니다.

- `127.0.0.1:20777`에서 F1 25 UDP를 받습니다.
- `127.0.0.1:22025`로 MOZA Pit House에 전달합니다.
- `127.0.0.1:8765`에 브라우저 HUD를 띄우고 기본 브라우저로 엽니다.
- F1 25 패킷이 감지되면 `PacketCarDamageData`를 MOZA가 읽기 쉬운 F1 24 호환 레이아웃으로 자동 변환합니다.
- MOZA 대시의 `TyreWear*` 값이 `100`에 고정되는 문제를 피하기 위해 CarDamage 호환 변환은 기본으로 켜져 있습니다.
- 알 수 없는 UDP 패킷은 파싱하지 않고 그대로 전달합니다.

브리지는 새 MOZA 키를 등록하지 않습니다. 예를 들어 Pit House가 `v1/gameData/BehindGap`을 제공하지 않는다면, 브리지만으로 그 키를 새로 만들 수는 없습니다.

## 옵션

| 옵션 | 기본값 | 설명 |
| --- | --- | --- |
| `--game` | `auto` | `auto`, `f1-25`, `generic-udp`, `lmu`, `ace`, `acr` 중 선택 |
| `--listen` | `20777` | F1 25 UDP를 받는 포트 |
| `--moza-port` | `22025` | MOZA Pit House로 전달할 포트 |
| `--debug` | `false` | 패치 로그와 초당 통계 출력 |

## 참고 문서

- [F1 25 / MOZA 텔레메트리 매트릭스](docs/f1-25-moza-telemetry-matrix.md)
- [MOZA 대시보드 바인딩 메모](docs/dashboard-binding.md)
- [게임 어댑터 조사](docs/game-adapter-research.md)

## 안전 메모

이 브리지는 F1 25로 입력을 되돌려 보내지 않습니다. UDP 텔레메트리를 읽고, 필요 시 MOZA Pit House로 전달합니다.

텔레메트리가 멈추면 다음 순서로 다시 시작합니다.

1. MOZA Pit House
2. Sim MOZA Bridge
3. F1 25
