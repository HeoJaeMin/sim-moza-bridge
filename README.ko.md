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
| UDP Format | `2026` |

MOZA Pit House는 F1 25 텔레메트리 입력으로 보통 `22025` 포트를 기대하므로, 브리지는 기본적으로 `20777`에서 받아 `22025`로 전달합니다.

브리지는 F1 25의 기본 `2025` 형식과 2026 Season Pack의 `2026` 형식을 모두 자동 감지합니다. 현재 Season Pack으로 주행할 때는 위 표처럼 `2026`을 선택합니다.

## LMU / ACE 설정

LMU와 ACE는 F1 25처럼 게임 안 UDP 포트만 맞추는 방식으로 이 브리지에 직접 연결되지 않습니다. Windows 게임 PC에서 각 게임을 실행한 뒤 해당 adapter를 선택합니다.

```bash
cargo run -- --game lmu
cargo run -- --game ace
```

- 기본 `cargo run`은 F1 UDP, `LMU_Data`, `Local\acevo_pmf_physics`를 함께 감시합니다. LMU를 하다가 F1 25를 시작해도 브리지를 껐다 켤 필요 없이 F1 UDP가 들어오면 자동으로 F1 경로를 우선합니다.
- `--game lmu`와 `--game ace`는 해당 adapter 고정 실행입니다.
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

실시간 레이스 엔지니어의 콘솔 무전을 켭니다. 기본 자동 감지 모드에서 F1 25, LMU, ACE 중 현재 주행 중인 게임의 공통 텔레메트리를 사용합니다.

```bash
cargo run -- --race-engineer
```

Windows 음성 무전, 이벤트 트리거와 병합 상태 이력 기록도 사용할 수 있습니다. 기본 명령은 코딩된 안전 규칙을 사용하는 `rules` 모드입니다.

```powershell
.\sim-moza-bridge.exe --engineer-voice --engineer-log race-engineer.jsonl --engineer-state race-engineer-live.json --engineer-history race-engineer-history.jsonl --engineer-trigger race-engineer-trigger.json
```

이 Codex 작업 자체가 지표를 보고 발화 여부와 콜을 결정하는 AI 모드는 다음처럼 실행합니다. AI 모드에서는 내장 규칙 문장을 발화하지 않고, 앞차·뒤차 간격과 순위 변화만으로 AI를 깨우지도 않습니다.

```powershell
$taskId = Read-Host 'AI 엔지니어로 사용할 Codex 작업 UUID'
.\sim-moza-bridge.exe --engineer-ai-hook C:\project\sim-moza-bridge\scripts\codex-ai-engineer-hook.ps1 --engineer-ai-task-id $taskId --engineer-state live-engineer\state.json
```

다음 레이스에서 다른 Codex 작업을 엔지니어로 사용할 때는 `$taskId`만 바꾸면 됩니다. 옵션을 생략하면 브리지를 시작한 현재 Codex 작업 ID를 사용합니다.

주행이 시작되면 랩과 순위, 전체 차량 피트 상태, SC/VSC, 날씨·예보와 피트 윈도, 연료·ERS, 타이어·손상을 감시합니다. 레이스에서는 클린 랩 페이스·마모 추세와 교통을 기록하고, F1 프랙티스에서는 현재 세팅값과 완료 랩을 함께 기록해 베이스라인, 단일 세팅 A/B 검증, 롱런과 퀄리파잉 시뮬레이션 순서의 주행 프로그램을 만듭니다. `--engineer-state`만 지정해도 같은 폴더에 이벤트 트리거와 상태 이력이 자동 생성됩니다. `--engineer-ai-hook`을 지정하면 의미 있는 텔레메트리 이벤트에서 현재 Codex 작업을 깨우고, AI 응답 뒤 최신 상태를 다시 검증한 뒤에만 음성을 냅니다. 자세한 지원 범위와 게임별 데이터 차이는 [실시간 레이스 엔지니어 문서](docs/race-engineer.md)에 정리되어 있습니다.

F1 UDP 입력을 CSV와 Markdown 분석 파일로 남길 수 있습니다.

```bash
cargo run -- --input-log inputs.csv --corner-log corners.csv --analysis-report analysis.md
```

브리지가 시작되면 로컬 HUD가 Rust native 창으로 열립니다. macOS에서는 macOS 창, Windows에서는 Windows 창으로 실행됩니다.

HUD에는 속도, 기어, RPM, REV LED, 입력 추적, 타이어/브레이크 상태, 앞차/뒤차/선두 gap, 연료, ERS, 손상 패널이 기본으로 표시됩니다.

## LMU 레이싱 대시보드

LMU용 트랙 맵, 전체 리더보드, 랩별 텔레메트리 저장, 접촉 기록을 한 화면에서 보는 별도 웹 대시보드를 실행할 수 있습니다.

```powershell
cargo run --bin lmu-dashboard -- --live
```

게임 없이 새 UI를 확인하려면 `--demo`를 사용합니다. 실행 방법, 태블릿 연결, 저장 위치와 접촉 판정의 한계는 [LMU 레이싱 대시보드 문서](docs/lmu-dashboard.md)에 정리되어 있습니다.

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
| `--input-log` | 없음 | F1 입력 샘플 CSV 누적 파일 |
| `--corner-log` | 없음 | 완료 랩 구간 요약 CSV 누적 파일 |
| `--analysis-report` | 없음 | 최신 완료 랩 분석 Markdown 파일 |
| `--race-engineer` | `false` | 게임 공통 실시간 콘솔 무전 |
| `--engineer-voice` | `false` | 레이스 엔지니어 활성화와 Windows 음성 무전 |
| `--engineer-log` | 없음 | 레이스 엔지니어 이벤트 JSON Lines 누적 파일 |
| `--engineer-state` | 없음 | AI 판단용 최신 병합 텔레메트리 JSON 파일. 트리거·이력을 같은 폴더에 자동 활성화 |
| `--engineer-history` | 없음 | 약 5Hz 병합 텔레메트리 상태 JSON Lines 이력 |
| `--engineer-trigger` | 없음 | 의미 있는 이벤트 발생 시 즉시 교체되는 AI 트리거 JSON |
| `--engineer-hook` | 없음 | 트리거 작성 직후 실행할 프로그램 또는 PowerShell 스크립트 |
| `--engineer-ai-hook` | 없음 | 현재 Codex 작업이 텔레메트리에서 무전을 직접 결정하는 AI 판단 훅 |
| `--engineer-ai-task-id` | `CODEX_THREAD_ID` | 이번 실행에서 AI 엔지니어로 사용할 Codex 작업 UUID |
| `--engineer-radio-hook` | 없음 | rules 모드에서 실제 TTS 재생 뒤 실행하는 레거시 사후 훅 |
| `--debug` | `false` | 패치 로그와 초당 통계 출력 |

## 참고 문서

- [F1 25 / MOZA 텔레메트리 매트릭스](docs/f1-25-moza-telemetry-matrix.md)
- [MOZA 대시보드 바인딩 메모](docs/dashboard-binding.md)
- [게임 어댑터 조사](docs/game-adapter-research.md)
- [LMU 레이싱 대시보드](docs/lmu-dashboard.md)
- [실시간 레이스 엔지니어](docs/race-engineer.md)

## 안전 메모

이 브리지는 F1 25로 입력을 되돌려 보내지 않습니다. UDP 텔레메트리를 읽고, 필요 시 MOZA Pit House로 전달합니다.

텔레메트리가 멈추면 다음 순서로 다시 시작합니다.

1. MOZA Pit House
2. Sim MOZA Bridge
3. F1 25
