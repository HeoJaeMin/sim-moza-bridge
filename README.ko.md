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

## LMU / ACE / ACR 설정

LMU, ACE, ACR은 F1 25처럼 게임 안 UDP 포트만 맞추는 방식으로 이 브리지에 직접 연결되지 않습니다. Windows 게임 PC에서 각 게임을 실행한 뒤 해당 adapter를 선택합니다.

```bash
cargo run -- --game lmu
cargo run -- --game ace
cargo run -- --game acr
```

- 기본 `cargo run`은 F1 UDP, `LMU_Data`, `Local\acevo_pmf_physics`를 함께 감시합니다. LMU를 하다가 F1 25를 시작해도 브리지를 껐다 켤 필요 없이 F1 UDP가 들어오면 자동으로 F1 경로를 우선합니다.
- `--game lmu`, `--game ace`, `--game acr`은 해당 adapter 고정 실행입니다.
- LMU adapter는 `LMU_Data` 공유 메모리를 읽고 HUD에 속도, RPM, 기어, 입력, 타이어/브레이크, 연료, 앞/뒤 차 gap을 표시합니다.
- LMU 대시보드는 활성 scoring row와 telemetry block 각각의 안정성 마커를 복사 전후로 검사합니다. 원본의 NaN/무한대와 순간 급변은 0으로 바꾸지 않고 품질 검사에서 제외·집계합니다.
- ACE adapter는 `Local\acevo_pmf_physics` 공유 메모리를 읽고 HUD에 기본 주행 텔레메트리를 표시합니다.
- ACR adapter는 `Local\acpmf_physics`, `Local\acpmf_graphics`, `Local\acpmf_static`을 읽어 주행 값과 스테이지 거리, 트랙/차량 정보를 결합합니다.
- ACR physics/graphics는 packet ID의 전후 일치를 확인합니다. packet ID가 없는 static은 전체 784 bytes를 연속 두 번 복사해 두 결과가 같은 경우에만 트랙/차량 context로 사용합니다.
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

F1 UDP 입력을 CSV와 Markdown 분석 파일로 남길 수 있습니다.

```bash
cargo run -- --input-log inputs.csv --corner-log corners.csv --analysis-report analysis.md
```

ACR에서 검증을 통과한 live 프레임은 `--archive-dir`을 지정했을 때 분석 주기와 별개로 스테이지별 zstd JSONL raw 큐에 전달됩니다. `--analysis-rate-hz`의 기본 25 Hz(설정 범위 20~50 Hz)는 코칭 분석과 선택적 legacy CSV에만 적용됩니다. 비동기 raw 큐가 가득 차 유실된 프레임 수는 스테이지 보고서와 품질에 반영됩니다.

완주, 중단, 리커버리와 다음 시도는 이벤트로 기록하고, 결과 화면 진입은 시도 결과를 다시 만들지 않는 별도 `result_screen` 이벤트로 기록합니다. 분석 JSON에는 다운샘플링된 시도 trace도 함께 저장하므로, 브리지를 재시작해도 같은 트랙·차량의 최근 시도 최대 32개를 복원해 이전 완주·실패와 비교합니다. `Ctrl-C` 또는 네이티브 HUD 창 닫기는 writer에 정상 종료를 전달하고 남은 queue를 flush한 뒤 zstd stream을 마감합니다. ACR과 LMU 분석은 `valid`, `partial`, `rejected`, `unknown`의 공통 4-state 품질 모델과 구조화된 사유를 사용합니다. LMU의 P1/집단 코칭 기준에는 품질 조건을 통과한 `valid` trace만 사용합니다. 원본과 분석 결과에는 각각 보존 기간을 적용합니다.

공유 메모리의 안정 복사, capture 카운터, 세션 identity/context, 텔레메트리 품질, 분석 신뢰도와 구조화된 한계는 library core에서 게임 어댑터가 함께 사용합니다. 상태 머신, 공식 이벤트 해석, 저장 schema와 게임별 코칭 알고리즘은 데이터 의미가 달라 어댑터별 orchestration으로 유지합니다.

```bash
cargo run -- --game acr --headless --archive-dir acr-telemetry-data
```

게임이 공식 완주 시간을 주지 않는 코스에서는 수동 결승 거리와 목표 시간을 지정할 수 있습니다.

```bash
cargo run -- --game acr --headless --archive-dir acr-telemetry-data \
  --acr-finish-distance-m 12450.5 --acr-target-time-s 418.25
```

기존 CSV가 꼭 필요할 때만 `--input-log acr-coaching.csv`를 명시적으로 추가합니다. CSV도 `--analysis-rate-hz`의 다운샘플링을 따릅니다.

브리지가 시작되면 로컬 HUD가 Rust native 창으로 열립니다. macOS에서는 macOS 창, Windows에서는 Windows 창으로 실행됩니다.

HUD에는 속도, 기어, RPM, REV LED, 입력 추적, 타이어/브레이크 상태, 앞차/뒤차/선두 gap, 연료, ERS, 손상 패널이 기본으로 표시됩니다.

## LMU 레이싱 대시보드

LMU용 트랙 맵, 전체 리더보드, 랩별 텔레메트리 저장, 접촉 기록을 한 화면에서 보는 별도 웹 대시보드를 실행할 수 있습니다. 활성 세션 동안 UI에는 클래스와 관계없이 전체 차량을 표시하지만, 완료 랩과 상세 샘플은 플레이어와 같은 클래스 차량만 저장합니다. 코칭 결과는 인게임 오버레이가 아니라 웹 대시보드와 별도 리포트에서 제공합니다.

```powershell
cargo run --bin lmu-dashboard -- --live
```

게임 없이 새 UI를 확인하려면 `--demo`를 사용합니다. 데모와 자동 테스트는 Windows 실게임 검증을 대신하지 않습니다. 실행 방법, 태블릿 연결, SQLite migration, 로컬 pause/resume/shutdown, Windows 확인 절차와 접촉 판정의 한계는 [LMU 레이싱 대시보드 문서](docs/lmu-dashboard.md)에 정리되어 있습니다.

## 동작 방식

브리지는 자동 감지 모드로 동작합니다.

- `127.0.0.1:20777`에서 F1 25 UDP를 받습니다.
- `127.0.0.1:22025`로 MOZA Pit House에 전달합니다.
- macOS/Windows에서는 브리지 프로세스 안에서 Rust native HUD 창을 띄웁니다.
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
| `--input-log` | 없음 | 입력 샘플 CSV 누적 파일. ACR은 코칭용 확장 채널을 기록 |
| `--corner-log` | 없음 | 완료 랩 구간 요약 CSV 누적 파일 |
| `--analysis-report` | 없음 | 최신 완료 랩 분석 Markdown 파일 |
| `--archive-dir` | 없음 | ACR의 검증된 live raw zstd와 JSON/Markdown 분석 저장 경로 |
| `--analysis-rate-hz` | `25` | ACR 코칭 분석 및 legacy CSV 주기. `20`~`50` Hz이며 raw zstd에는 적용하지 않음 |
| `--raw-retention-days` | `7` | ACR zstd 원본 보존 일수 |
| `--analysis-retention-days` | `90` | ACR JSON/Markdown 분석 보존 일수 |
| `--acr-finish-distance-m` | 없음 | 공식 결과 신호가 없을 때 사용할 수동 결승 거리(m) |
| `--acr-target-time-s` | 없음 | 랠리 코칭 목표 시간(초) |
| `--headless` | `false` | 네이티브 HUD를 열지 않고 수집기만 실행 |
| `--debug` | `false` | 패치 로그와 초당 통계 출력 |

## 참고 문서

- [F1 25 / MOZA 텔레메트리 매트릭스](docs/f1-25-moza-telemetry-matrix.md)
- [MOZA 대시보드 바인딩 메모](docs/dashboard-binding.md)
- [게임 어댑터 조사](docs/game-adapter-research.md)
- [LMU 레이싱 대시보드](docs/lmu-dashboard.md)

## 안전 메모

이 브리지는 F1 25로 입력을 되돌려 보내지 않습니다. UDP 텔레메트리를 읽고, 필요 시 MOZA Pit House로 전달합니다.

텔레메트리가 멈추면 다음 순서로 다시 시작합니다.

1. MOZA Pit House
2. Sim MOZA Bridge
3. F1 25
