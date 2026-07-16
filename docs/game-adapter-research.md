# ACE / ACR / LU 어댑터 조사

확인일: 2026-07-15

이 문서의 약어:

- `ACE`: Assetto Corsa EVO
- `ACR`: Assetto Corsa Rally
- `LU` / `LMU`: Le Mans Ultimate

## 요약

ACE, ACR, LU는 F1 25 UDP 프로필처럼 취급하면 안 됩니다. 구현 구조는 다음이 맞습니다.

```text
게임 프로세스 / 공유 메모리 / 플러그인
  -> 게임별 어댑터
  -> 정규화된 텔레메트리 모델
  -> 로컬 HUD
```

`generic-udp`는 다른 익스포터가 대상 앱이 이해할 수 있는 패킷을 이미 만들어줄 때만 유효합니다. 네이티브 ACE, ACR, LU 파싱을 대체하지 않습니다.

## 조사 결과

| 게임 | 현재 공개 신호 | 브리지 구현 의미 |
| --- | --- | --- |
| ACE | MOZA는 텔레메트리 지원으로 표시합니다. Assetto Corsa EVO 0.6 릴리스 노트는 공유 메모리 라이브러리 갱신과 공식 MoTeC 지원을 언급합니다. 일부 서드파티 대시보드 문서는 얼리 액세스 기본 값만 노출된다고 설명합니다. | `Local\acevo_pmf_physics`를 읽는 1차 Windows 공유 메모리 어댑터가 있습니다. 얼리 액세스 업데이트마다 필드 범위가 바뀔 수 있습니다. |
| ACR | 실행 중인 ACR에서 ACC 호환 이름의 physics/graphics/static 파일 매핑과 갱신되는 주행 값을 직접 확인했습니다. 공개 ACR Telemetry Recorder도 같은 세 매핑을 직접 읽습니다. | `Local\acpmf_*`를 읽는 Windows 어댑터가 구현되어 있습니다. MOZA 키 범위는 별도 검증 대상입니다. |
| LU / LMU | MOZA는 텔레메트리 지원으로 표시하고, MOZA Digital Dash 키 매트릭스에는 `Le mans ultimate` 컬럼이 있습니다. LMU에는 오프라인 분석용 DuckDB 텔레메트리 기록도 있습니다. 일부 서드파티 대시보드는 플러그인 기반 실시간 경로를 사용할 수 있습니다. | `LMU_Data`를 읽는 1차 Windows 공유 메모리 어댑터가 있습니다. DuckDB 기록은 분석 가져오기에는 유용하지만 지연 시간이 낮은 실시간 HUD 경로가 아닙니다. |

## MOZA 키 지원 범위

MOZA 게임 호환 목록은 세 게임 모두 텔레메트리 지원으로 표시합니다.

- Assetto Corsa EVO: 텔레메트리 지원
- Assetto Corsa Rally: 텔레메트리 지원
- Le Mans Ultimate: 텔레메트리 지원

MOZA Digital Dash Telemetry Support 표는 다른 의미입니다. 이 표는 키별 대시보드 매트릭스입니다. 현재 표 기준:

| 컬럼 | 표의 지원 키 수 | 비고 |
| --- | ---: | --- |
| `Assetto Corsa Competizione` | 105 | AC 계열 참고용. ACR 범위 증거는 아님 |
| `Assetto Corsa` | 98 | AC 계열 참고용. ACE/ACR 범위 증거는 아님 |
| `Le mans ultimate` | 105 | LU/LMU 대시보드 키 범위의 직접 증거 |
| `Assetto Corsa EVO` | 없음 | 전용 Digital Dash 컬럼 없음 |
| `Assetto Corsa Rally` | 없음 | 전용 Digital Dash 컬럼 없음 |

LU는 속도/RPM/기어/입력, 랩/차간/타이밍, 연료, 타이어/브레이크 온도, 타이어 웨어, 타이어 압력, 피트/플래그 상태, 트랙/세션 메타데이터, 차량 좌표, RPM 퍼센트, 휠 스핀, 트랙 위치 퍼센트, 위치, 플레이어 인덱스, 상대 차량 수 그룹이 확인됩니다.

ACE와 ACR은 MOZA 일반 텔레메트리 지원이 있으므로 Pit House가 일부 텔레메트리를 받을 수는 있습니다. 하지만 공개 Digital Dash 표는 두 게임에서 어떤 `v1/gameData/...` 키가 채워지는지 아직 보여주지 않습니다. 따라서 실제 Pit House 캡처로 확인하기 전까지는 불확실한 값을 로컬 HUD/로깅 계층에 보관해야 합니다.

오늘 기준 확정 매핑 하위 집합은 [confirmed-telemetry-mappings.md](confirmed-telemetry-mappings.md)에 유지합니다.

## 어댑터 메모

### ACE

확인된 방향:

- 단순 UDP 스트림이 아닙니다.
- 공식 업데이트 노트는 공유 메모리와 MoTeC 출력을 언급합니다.
- 얼리 액세스 상태라 텔레메트리 구조와 필드 범위가 바뀔 수 있습니다.
- 일부 서드파티 대시보드 문서는 기본 데이터 값만 노출된다고 설명합니다.

현재 구현:

```text
ACE 공유 메모리
  -> ACE 어댑터
  -> 정규화된 텔레메트리
  -> HUD
```

비공식 예시에서 관찰된 공유 메모리 이름:

```text
Local\acevo_pmf_physics
Local\acevo_pmf_graphics
Local\acevo_pmf_static
```

현재 adapter는 `Local\acevo_pmf_physics`의 기본 주행 필드를 읽습니다. 설치된 게임 버전에서 구조가 바뀌면 파서 offset을 다시 검증해야 합니다.

### ACR

확인된 방향:

- MOZA는 ACR 텔레메트리를 지원으로 표시합니다.
- SimHub는 ACR을 지원 게임으로 표시합니다.
- 공개 ACR Telemetry Recorder는 ACR과 ACC의 공유 메모리를 직접 읽습니다.
- 2026-07-15 라이브 검증에서 다음 매핑과 크기 이상의 유효 데이터를 확인했습니다.

```text
Local\acpmf_physics  800 bytes
Local\acpmf_graphics 1588 bytes
Local\acpmf_static   784 bytes
```

현재 구현:

```text
ACR ACC 호환 공유 메모리
  -> 안정 프레임 판독 + ACR 상태 머신
  -> 정규화된 텔레메트리
  -> HUD / 검증된 live raw zstd / 20~50Hz 분석 및 legacy CSV
```

확인 및 기록하는 값:

- 속도, RPM, 기어, 스로틀, 브레이크, 클러치, 조향
- 스테이지 거리와 트랙 길이, 트랙/차량 이름
- G, 월드/로컬 속도, 로컬 각속도, 자세
- 휠 슬립/하중/회전속도, 슬립 비율/각도
- 서스펜션 이동/손상, 타이어 압력/온도, 브레이크 온도/압력
- 연료, 브레이크 바이어스, ABS/TC 상태, FFB, 엔진 상태

세 매핑은 연결 가능한 동안 열린 상태로 유지합니다. physics와 graphics는 packet ID를 복사 전후에 비교하고 snapshot 안의 ID까지 일치할 때만 사용합니다. static에는 packet ID가 없으므로 전체 784 bytes를 연속 두 번 복사해 두 결과가 같을 때만 사용하며, 복사 중 값이 계속 바뀌면 해당 refresh를 버립니다. 정지한 graphics와 새 physics를 결합하지 않고, RPM, 속도, G와 거리의 비정상 값이나 순간 급변도 정규화 전 단계에서 제외합니다.

ACR 상태 머신은 `Idle`, `Countdown`, `Running`, `Finished`, `Aborted`, `Recovery`와 다음 시도를 구분합니다. 단순 거리 감소만으로 새 주행을 만들지 않으며, 공식 완료 이벤트와 완료 시간을 우선합니다. 결과 화면 진입은 완주/중단 결과를 다시 만들지 않고 별도 `result_screen` 이벤트로 한 번 기록합니다. 공식 결과가 없는 코스는 `--acr-finish-distance-m`, 목표/메달 비교는 `--acr-target-time-s`로 지정할 수 있습니다.

ACR 라이브 값에서 공기/노면/냉각수/타이어/브레이크 온도는 Kelvin으로 관찰되어, 공기 온도로 단위를 판별한 뒤 섭씨로 기록합니다. `--archive-dir`을 지정하면 검증을 통과한 각 live 프레임을 스테이지별 `.jsonl.zst` raw 큐에 전달합니다. 이 raw 경로는 `--analysis-rate-hz`로 다운샘플링하지 않습니다. 큐가 가득 차면 일반 raw 프레임은 drop될 수 있으며 그 stage별 수를 품질과 보고서에 남깁니다. stage 이벤트는 우선 전달하고 encoder는 주기적 또는 stage 종료 시 flush합니다. `Ctrl-C`와 네이티브 HUD 창 닫기 때도 writer가 queue를 비우고 zstd stream을 마감할 때까지 join합니다.

기본 25 Hz(`--analysis-rate-hz 20..50`)는 코칭용 trace와 `--input-log`로 명시한 legacy CSV에만 적용됩니다. stage 종료 시 다운샘플링된 trace를 포함한 UTF-8 JSON과 Markdown 분석 보고서를 저장하고, 다음 실행에서는 같은 트랙·차량의 최근 trace 최대 32개를 복원합니다. raw와 분석 파일의 기본 보존 기간은 각각 7일과 90일입니다. 분석 보고서는 LMU 랩과 같은 `valid`, `partial`, `rejected`, `unknown` 4-state 품질 모델을 사용하며, 커버리지·시간 일치·샘플 간격·역행·비정상 값과 validator/archive drop을 사유로 남깁니다.

### LU / LMU

확인된 방향:

- MOZA는 LU 텔레메트리를 지원으로 표시합니다.
- MOZA Digital Dash 키 매트릭스에는 `Le mans ultimate` 컬럼이 직접 있습니다.
- 공식 LMU 텔레메트리 기록은 오프라인 분석용 DuckDB 파일을 내보냅니다.
- 서드파티 실시간 대시보드는 공유 메모리/플러그인 기반 연동을 사용할 수 있습니다.

현재 구현:

```text
LMU_Data 공유 메모리
  -> LMU 어댑터
  -> 정규화된 텔레메트리
  -> HUD
```

첫 LU 어댑터 구현 범위:

- 속도/RPM/기어/입력
- 랩 타이밍과 차간
- 타이어 웨어, 타이어 온도, 타이어 압력
- 브레이크 온도와 브레이크 바이어스
- 연료와 연료 용량
- 피트 리미터

별도 `lmu-dashboard`는 활성 LMU 세션의 scoring/telemetry를 함께 읽습니다. scoring count와 활성 scoring row, telemetry count와 활성 telemetry row의 marker를 모두 복사 전후로 확인하며, 하나라도 복사 중 바뀌면 그 frame을 다시 읽거나 거절합니다. 지도와 리더보드는 클래스와 관계없이 전체 차량을 표시하지만, 완료 랩과 20 Hz 상세 샘플은 플레이어와 같은 클래스 차량만 세션 전체에서 추적·저장합니다. 다른 클래스의 상세 텔레메트리는 SQLite에 저장하지 않습니다. 원본 NaN/무한대나 순간 급변은 정규화 전에 거절하고 별도 telemetry 카운터와 랩 품질에 반영합니다. 코칭도 같은 클래스의 품질 조건을 통과한 `valid` 랩을 기준으로 합니다. 세션 경계에서는 진행 랩을 `partial`로 flush하고, 0.5초보다 큰 세션 시간 초기화 또는 track/session type 변경만 새 세션으로 판정합니다. migration, 로컬 수집 제어와 Windows 확인 절차는 [lmu-dashboard.md](lmu-dashboard.md)에 별도로 유지합니다.

안정 복사, capture 카운터, 세션 identity/context, 4-state 품질, 분석 신뢰도와 구조화된 한계는 공통 library core를 사용합니다. 상태 머신, 공식 이벤트 해석, 저장 schema와 게임별 코칭 알고리즘은 LMU와 ACR의 완료/랩 의미가 달라 각 adapter orchestration에 남겨 둡니다.

남은 확장 후보:

- 피트레인/플래그 상태
- 트랙 위치 퍼센트와 차량 좌표

## 추가 검증 작업

Windows PC에서 실제 게임 세션을 실행한 상태로 확인해야 합니다.

1. ACR 게임 업데이트 후 세 매핑 크기와 핵심 오프셋 회귀 확인
2. Countdown → Running → Finished와 실패 → Recovery → 다음 시도 전환을 실제 게임에서 대조
3. ACR 공식 완료와 `result_screen`이 각각 한 번 기록되고 결과 화면이 새 시도 결과를 만들지 않는지 확인
4. ACR 저장 보고서 시간이 공식 시간 또는 수동 결승의 `current_time_ms`와 일치하는지 확인
5. 검증된 live frame 수, raw zstd 기록 수와 `archive_backpressure_drops`를 비교하고 분석/CSV만 설정 Hz로 줄어드는지 확인
6. 일시정지 후 재개할 때 stale graphics 거부가 정상 주행 프레임을 과도하게 버리지 않는지 확인
7. Pit House가 ACE/ACR 값을 기존 `v1/gameData/...` 키로 노출하는지 확인
8. LU 키 값이 MOZA Digital Dash 표와 일치하는지 확인
9. LMU 멀티클래스 세션에서 전체 클래스 UI와 동일 클래스 상세 저장 경계, migration, pause/resume/shutdown flush를 확인
10. REV 라이트와 휠 LED가 RPM 퍼센트, REV 라이트 플래그, 하드웨어별 MOZA 연동 중 무엇으로 구동되는지 확인

## 출처

- MOZA Game Compatibility List: https://support.mozaracing.com/en/support/solutions/articles/70000629729-game-support-list
- MOZA Digital Dash Telemetry Support: https://support.mozaracing.com/en/support/solutions/articles/70000627978-digital-dash-telemetry-support
- Assetto Corsa EVO 0.6 release notes: https://assettocorsa.gg/assetto-corsa-evo-early-access-06-now-available/
- SIM Dashboard Assetto Corsa EVO notes: https://www.stryder-it.de/simdashboard/help/en/For_PC_Gamers/Game_Configuration/Assetto_Corsa_EVO
- Racing Overlay ACR telemetry support notes: https://luizzak.itch.io/racing-overlay/devlog/1321475/assetto-corsa-rally-telemetry-support
- ACR Telemetry Recorder source: https://github.com/decnet100/acr_telemetry
- SimHub supported games: https://www.simhubdash.com/supported-games/
- Le Mans Ultimate Telemetry Recording: https://guide.lemansultimate.com/hc/en-gb/articles/14524956311695-Telemetry-Recording
- SIM Dashboard Le Mans Ultimate notes: https://www.stryder-it.de/simdashboard/help/en/For_PC_Gamers/Game_Configuration/Le_Mans_Ultimate
- goLMUSharedMemory API docs: https://pkg.go.dev/github.com/stephenhoran/goLMUSharedMemory
