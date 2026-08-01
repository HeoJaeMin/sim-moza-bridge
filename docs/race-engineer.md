# 실시간 레이스 엔지니어

`sim-moza-bridge`의 레이스 엔지니어는 게임별 원본 패킷을 직접 판단하지 않고 공통 `TelemetryUpdate`를 사용합니다. 따라서 F1 25 UDP, LMU 공유 메모리, ACE 공유 메모리 중 현재 감지된 소스가 바뀌어도 같은 판단 루프가 동작합니다.

## 실행

콘솔 무전:

```powershell
.\sim-moza-bridge.exe --race-engineer
```

Windows 음성 무전:

```powershell
.\sim-moza-bridge.exe --engineer-voice
```

음성 무전, 상태 이력, 이벤트 트리거 기록:

```powershell
.\sim-moza-bridge.exe `
  --engineer-voice `
  --engineer-log race-engineer.jsonl `
  --engineer-state race-engineer-live.json `
  --engineer-history race-engineer-history.jsonl `
  --engineer-trigger race-engineer-trigger.json
```

`--engineer-*` 출력 옵션은 레이스 엔지니어를 자동으로 활성화합니다. 게임을 지정하지 않은 기본 `auto` 모드에서는 F1 25, LMU, ACE를 함께 감시하며 실행 중인 소스를 자동 선택합니다.

Windows 음성 출력은 운영체제의 `System.Speech`를 사용합니다. 한국어 음성이 설치되어 있으면 우선 선택하고, 없으면 Windows 기본 음성을 사용합니다. 음성 프로세스를 시작할 수 없더라도 콘솔 무전은 계속 동작합니다.

## 현재 무전 항목

- 차량이 실제로 움직이기 시작했을 때 엔지니어 연결
- 랩 무효와, 랩 분석 뒤 실제 다음 조치가 달라졌을 때의 프로그램·세팅 콜
- 세이프티카·VSC 전개와 종료, 레이스 재시작 준비
- 현재 날씨 변화와 세션 예보의 강우 시작·건조 전환
- 앞차·뒤차 식별과 간격 기록. 추월·순위 교체 때 불안정한 분류 간격은 원본 상태와 분석에만 남기고 선제 음성으로 읽지 않음
- 앞뒤 경쟁차의 피트 진입, 게임 피트 윈도와 예상 복귀 순위
- 클린 랩 추세를 이용한 타이어 열화, 연료 목표, ERS 관리와 레이스 종료 직전 스테이 아웃 판단
- 타이어 마모·손상
- 프런트 윙, 리어 윙, 기어박스, 엔진 손상
- 피트 리미터 작동·해제
- 현재 마셜 구간의 옐로·레드·그린 플래그
- 최종 분류 패킷을 이용한 완주·DNF·실격과 최종 순위

레이스 중 같은 업데이트에서 서로 다른 중요 콜이 함께 발생하면 모두 음성 큐에 보존합니다. 재생 순서는 레이스 컨트롤·손상, 전략·자원, 전투 간격 순이며 같은 종류의 계속 바뀌는 상태만 최신 값으로 병합합니다. 순위 변화와 10랩 단위 전략 스냅샷은 상태와 이력에는 남지만 그 자체로 음성을 만들지 않으며, 기존의 5랩 주기 일반 스틴트 무전은 제거했습니다.

같은 경고는 매 프레임 반복하지 않습니다. F1의 앞뒤 간격은 추월과 순위 교체 구간에서 0.0~0.2초로 순간 수렴하거나 상대가 바뀌는 분류용 값이므로 공격·방어 음성의 근거로 사용하지 않습니다. 같은 상대 차량과 순위에서 1초 동안 확인하는 내부 상태는 이벤트 감사에만 유지하고, 간격과 선택한 슬롯 ID는 `car_in_front_index`·`car_behind_index`로 상태와 이력에만 남깁니다. SC/VSC 여부와 무관하게 일반 앞차·뒤차 간격은 선제 음성이나 발화 후 AI 훅을 만들지 않습니다. 라이벌 피트 판단은 레이스 세션에서 동일 랩이고 10초 이내인 분류상 앞뒤 차량에만 적용하며, SC/VSC 중 피트는 언더컷으로 단정하지 않고 트랙 포지션 재검토 콜로 바꿉니다. Lap·Session·전체 순위 패킷은 종류별 `overall frame identifier`를 기준으로 오래된 패킷을 폐기하고, 해당 값이 없는 게임에서는 세션 시간과 패킷 프레임 번호를 함께 사용합니다. 따라서 역순 UDP 때문에 SC 종료나 피트 진입이 왕복하지 않으면서 F1 플래시백은 새 타임라인으로 정상 수용합니다. 차량이 15초 이상 정지하면 다음 주행 시작을 기다립니다. F1은 `sessionUID`가 실제로 바뀔 때만 새 세션으로 초기화하므로 일시정지 때문에 연결 무전과 랩 결과가 중복되지 않습니다. 브리지를 재시작해도 기존 `state.json`의 UID와 들어온 UID가 같으면 연결 무전을 다시 내보내지 않습니다.

레이스 전략 추세는 피트 랩, 무효 랩, SC/VSC 랩을 제외한 같은 스틴트의 클린 랩을 최소 6개 모은 뒤 강건한 중앙 기울기로 계산합니다. 명백한 느린 랩은 표본에서 제외합니다. 연료 부족은 목표 대비 `-0.3랩` 미만이 2개 그린 클린 랩 연속일 때, ERS 관리는 저장량 10% 미만이 3개 그린 클린 랩 연속일 때만 알립니다. ERS가 20% 이상으로 회복되어야 같은 경고가 다시 준비됩니다. 새 스틴트가 시작되면 다음 피트 윈도를 다시 안내합니다. 마지막 1~3랩의 스테이 아웃은 건조·그린, 10분 내 강우 위험 없음, 연료 여유, 예상 결승 마모 70% 미만, 타이어 손상·블리스터 없음, 두 종류 드라이 컴파운드 의무 충족, 피트 시 예상 순위 손실을 모두 확인한 경우에만 제안합니다. 이후 SC·날씨·손상·연료가 바뀌면 기존 계획을 취소하고 재검토 콜을 냅니다.

Windows 음성 큐는 순위·앞차 갭·뒤차 갭·플래그처럼 계속 바뀌는 상태를 종류별 최신 한 건으로 병합합니다. 긴 일반 무전 재생 중 SC·적기 같은 긴급 콜이 들어오면 현재 TTS 프로세스를 중단하고 긴급 콜을 먼저 처리합니다. 비긴급 무전은 브레이크 20% 이상 또는 큰 조향 중에는 대기하고 안전 구간에서만 시작합니다. TTS는 발화마다 격리된 프로세스로 실행해 한 번 실패해도 다음 콜에서 다시 시작하며, 한국어 음성이 없으면 Windows 기본 음성을 사용합니다. 발화 직전에 소스, 세션 UID, 타임라인 revision, 손상·전략·라이벌 상태 revision을 다시 확인하므로 이미 해제되거나 수리된 콜은 읽지 않습니다. 일시적인 콜은 5초, 프로그램·세션 종료·손상 콜은 20초, 현재 상태와 revision으로 검증되는 긴급 콜은 최대 60초가 지나면 폐기됩니다.

같은 UID에서 세션 시간이 1초 넘게 역행하면 플래시백으로 처리합니다. 대기 중인 이전 타임라인 무전을 즉시 무효화하고 `timeline_reset` 이벤트에 되감기 전후 시간과 새 `timeline_revision`을 기록합니다. 코너 분석도 되감기 이후에 존재하지 않는 trace point를 제거하므로 사고 전 분기와 최종 주행 분기가 한 랩에 합산되지 않습니다.

프랙티스에서는 랩마다 랩타임과 남은 프로그램 랩 수를 반복해서 읽지 않습니다. 프로그램 단계, 목표 또는 첫 실행 조치가 바뀔 때만 짧은 무전을 재생합니다. 랩 완료 이벤트와 전체 지표는 분석·이력용으로 계속 기록되지만 음성에는 필요한 결론만 포함합니다.

마셜 구간의 블루 플래그 값은 플레이어 개인에게 내려온 지시가 아니므로 음성 콜에서 제외합니다. 연료 `m_fuelRemainingLaps`는 남은 연료량이 아니라 MFD의 목표 대비 랩 델타로 취급합니다. 이 절의 내장 콜은 기본 `rules` 모드에서만 발화합니다. 아래의 AI 판단 모드에서는 텔레메트리 정규화와 상태 변화 감지에만 사용되고, 실제 발화 여부·내용·우선순위는 AI가 결정합니다.

## 이벤트 기반 AI 엔지니어

실제 AI 판단 모드는 `--engineer-ai-hook <path>`로 켭니다. 이 모드의 흐름은 다음과 같습니다.

1. Rust 런타임이 게임별 데이터를 공통 `TelemetryUpdate`로 정규화하고 상태·이력을 기록합니다.
2. 랩 완료, 레이스 컨트롤, 손상, 연료·ERS, 라이벌 피트처럼 판단할 이유가 생겼을 때만 불변 트리거 스냅샷으로 AI를 깨웁니다.
3. AI가 원본 지표와 최근 랩 근거를 보고 `발화` 또는 `침묵`, 메시지, 우선순위와 유효 시간을 직접 결정합니다.
4. 응답 후 최신 `state.json`을 다시 읽어 세션 UID, 타임라인 revision, 주행 상태, 레이스 컨트롤 상태와 TTL을 검증한 뒤에만 음성을 재생합니다.

AI 모드에서는 Rust 규칙이 만든 문장을 무전하지 않습니다. 콘솔에는 상태 변화가 `[engineer-observation]`으로만 남고, 내장 `--engineer-voice` 및 사후 `--engineer-radio-hook`도 비활성화됩니다. 따라서 코딩된 후보 문장을 AI가 다듬는 구조가 아니라, AI가 텔레메트리 근거에서 콜 자체를 결정합니다. 순위·앞차·뒤차 간격 변화와 정기 전략 스냅샷만으로는 AI를 깨우지 않으며, 불안정했던 앞뒤 간격 값은 AI 입력과 발화 모두에서 제외합니다.

저장소의 `scripts/codex-ai-engineer-hook.ps1`은 브리지를 시작한 현재 Codex 작업의 `CODEX_THREAD_ID`를 `codex exec resume --ephemeral`로 재개합니다. 별도의 독립 AI 작업을 만들지 않고 이 작업의 문맥을 판단에 사용하되, 각 내부 판단을 대화 기록에 계속 쌓지는 않습니다. AI 응답은 엄격한 `scripts/ai-engineer-decision.schema.json`으로 제한하며, 아무 콜도 필요 없으면 `speak=false`를 반환합니다. 판단 TTL은 모델 왕복 시간을 포함한 20~30초이고, 그 안에도 관련 상태 revision이 바뀌면 즉시 폐기합니다. 실제 발화만 `ai-engineer-radio.jsonl`에 기록되고 훅 처리 결과는 `codex-ai-engineer-hook.log`와 `ai-engineer-state.json`에 남습니다.

```powershell
$taskId = Read-Host 'AI 엔지니어로 사용할 Codex 작업 UUID'
.\sim-moza-bridge.exe `
  --engineer-ai-hook C:\project\sim-moza-bridge\scripts\codex-ai-engineer-hook.ps1 `
  --engineer-ai-task-id $taskId `
  --engineer-state live-engineer\state.json `
  --engineer-history live-engineer\history.jsonl `
  --engineer-trigger live-engineer\trigger.json
```

`--engineer-ai-task-id <UUID>`는 이번 실행에서 AI 엔지니어를 요청한 Codex 작업을 지정합니다. 다음 레이스에서는 이 값만 다른 작업 ID로 바꾸면 됩니다. 브리지는 이 값을 `SIM_MOZA_ENGINEER_TASK_ID`로 훅에 전달하며, 직접 훅을 실행할 때의 두 번째 인수가 가장 우선합니다. 옵션을 생략하면 훅은 현재 환경의 `CODEX_THREAD_ID`를 사용합니다. AI 훅만 지정한 경우에도 상태는 `engineer-state.json`, 이력은 `engineer-history.jsonl`, 트리거는 `engineer-trigger.json`으로 자동 활성화됩니다.

`--engineer-trigger <path>`는 주행 시작, 랩 완료·무효, 순위와 전투 간격 변화, 피트 리미터, 손상, 플래그, 최종 분류처럼 구조적인 변화가 생긴 순간에 최신 JSON 파일을 교체합니다. 트리거에는 원인이 된 관찰과 함께 그 순간의 입력, 랩, 세션, 연료, ERS, 타이어, 손상 전체 스냅샷이 들어갑니다. `decision_mode`는 `ai` 또는 `rules`이고 현재 트리거 스키마 버전은 4입니다.

기존 실행 명령과의 호환을 위해 `--engineer-state <dir>\state.json`만 지정해도 같은 폴더의 `engineer-trigger.json`과 `engineer-history.jsonl`을 자동으로 활성화합니다. 파일명을 바꾸고 싶을 때만 각각의 옵션을 명시하면 됩니다.

`--engineer-hook <path>`는 AI 계약이 없는 범용 이벤트 알림용 호환 옵션입니다. 트리거 파일을 쓴 직후 실행 파일이나 PowerShell 스크립트를 한 번 호출하며 첫 번째 인수와 `SIM_MOZA_ENGINEER_TRIGGER` 환경 변수로 경로를 전달합니다. 실행 중인 훅에는 sequence별 불변 스냅샷을 전달하고, 그동안 도착한 이벤트는 최신 sequence 한 건으로 병합합니다. 제한 시간은 30초입니다. `--engineer-ai-hook`과 함께 지정하면 AI 훅이 우선합니다.

`--engineer-radio-hook <path>`는 기본 `rules` 모드의 레거시 사후 처리 옵션입니다. 내장 `System.Speech`가 실제 재생을 끝내고 `engineer-radio.jsonl`을 flush한 뒤에만 실행됩니다. 이것은 AI가 콜을 결정하는 경로가 아니며 `--engineer-ai-hook` 모드에서는 실행되지 않습니다. 주행 도중 브리지를 연결한 경우 최초 손상 패킷은 기준 상태로만 저장하고, 이후 실제 악화만 새 이벤트로 처리합니다.

## 프랙티스 프로그램과 세팅 A/B 테스트

F1 세션 유형이 프랙티스이면 병합 상태에 `practice_program`을 추가합니다. 현재 타이어 사용 랩, 연료, 잔여 시간, 브리지 연결 후 확보된 클린 랩과 현재 세팅으로 다음 단계를 선택합니다.

- 사용 중인 타이어 스틴트의 롱런 표본 확보
- 동일 조건 베이스라인 2랩
- 한 번에 한 항목만 바꾸는 세팅 A/B 검증 2랩
- 레이스 페이스·타이어 열화 런
- 세션 후반 저연료 퀄리파잉 시뮬레이션

2025와 2026 Season Pack의 `PacketCarSetupData`를 모두 읽습니다. `state.json`의 `setup`에는 윙, 디퍼렌셜, 캠버·토, 서스펜션·안티롤바, 차고, 브레이크 압력·바이어스, 엔진 브레이킹, 세팅 타이어 압력과 밸러스트가 기록됩니다. `tyre_sets`에는 실제 사용 가능한 20개 타이어 세트의 컴파운드, 마모, 잔여 수명과 장착 상태가 들어갑니다. 피트에서는 이 재고를 기준으로 새 미디엄을 우선한 레이스 컴파운드 베이스라인을 제안합니다. 완료 랩 트리거에는 당시 세팅과 `recommendations`가 함께 들어가므로 AI가 랩타임 하나만 보지 않고 코너 입력, 타이어 상태와 세팅 변경 전후를 비교할 수 있습니다.

추천은 여러 항목을 동시에 바꾸지 않습니다. 예를 들어 프런트 제한 신호가 반복되면 현재 앞 윙 값에서 한 클릭만 변경하고, 나머지 조건을 유지한 두 개의 클린 랩으로 검증하도록 제안합니다. 주행 입력으로 구분하기 어려운 문제는 바로 세팅을 바꾸지 않고 먼저 드라이빙 A/B 랩을 요구합니다.

`--engineer-state`를 사용하면 같은 폴더의 `practice-advisor.json`에 세션 UID, 완료 랩, 세팅 서명과 마지막 변경 후보를 저장합니다. 브리지를 재시작해도 완료한 롱런을 다시 지시하지 않으며, 변경 세팅의 클린 랩 두 개를 확보한 뒤에는 추가 변경을 연속 제안하지 않고 A/B 결과 리뷰 단계로 전환합니다.

## 게임별 데이터 차이

레이스 엔지니어 로직은 공통이지만 실제 무전 범위는 각 게임 adapter가 제공하는 값에 따라 달라집니다.

| 게임 | 입력 | 현재 사용 가능한 주요 엔지니어 데이터 |
| --- | --- | --- |
| F1 25 | UDP 2025 / 2026 Season Pack | 전체 차량 순위·피트 상태, 랩, 앞뒤/선두 갭, SC/VSC, 날씨·예보, 게임 피트 윈도·복귀 순위, 연료량·목표 대비 델타, ERS 저장·배포·MGU-K/H 회수량·2026 회수 한도, 타이어, 손상, 차량 세팅, 프랙티스 프로그램, 피트 리미터, 마셜 플래그, 최종 분류 |
| LMU | `LMU_Data` 공유 메모리 | 주행 상태, 랩 번호, 앞뒤 갭, 연료량, 타이어 마모, 피트 리미터 |
| ACE | `Local\acevo_pmf_physics` 공유 메모리 | 주행 상태, 연료량, 입력 및 차량 기본 상태 |

`generic-udp`는 알 수 없는 패킷을 그대로 전달하는 모드이므로 내용을 파싱할 수 없고 엔지니어 판단도 만들 수 없습니다. 이후 새 게임 adapter가 `TelemetryUpdate`를 채우면 별도의 엔지니어 구현 없이 공통 콜을 사용할 수 있습니다.

## JSON Lines 출력

`--engineer-log` 파일에는 무전 한 건당 JSON 객체 한 줄이 추가됩니다.

```json
{"schema_version":2,"timestamp_unix_ms":1753848000000,"source":"f1-25","session_uid":1234,"session_time":300.0,"timeline_revision":0,"priority":"important","kind":"behind_gap","message":"뒤차 0.8초. 방어 거리."}
```

이 출력은 별도 음성 시스템, 방송 오버레이 또는 AI 전략 서비스와 연결할 수 있습니다. `priority`는 `normal`, `important`, `critical` 중 하나입니다.

`--engineer-state <dir>\state.json`과 `--engineer-voice`를 함께 사용하면 같은 폴더에 `engineer-radio.jsonl`도 생성됩니다. 이 파일은 후보 콜 전체가 아니라 `System.Speech` 재생이 정상적으로 끝난 무전만 기록합니다. 세션 UID, 세션 유형, 랩, 위치, 콜 종류와 실제 메시지를 포함하므로 대화형 요약은 이 파일의 새 행만 읽으면 됩니다.

```json
{"schema_version":2,"queued_at_unix_ms":1753848000000,"spoken_at_unix_ms":1753848003150,"source":"f1-25","session_uid":1234,"timeline_revision":0,"state_revision":3,"session_type":2,"lap":5,"position":1,"priority":"normal","kind":"practice_program","message":"다음 프로그램: 단일 세팅 변경 검증. 온스로틀 디퍼렌셜 50에서 45."}
```

## AI 판단용 상태와 이력

`--engineer-state`는 최신 병합 텔레메트리 스냅샷을 약 5Hz로 안전하게 교체합니다. 중요한 이벤트와 최종 분류에서는 5Hz 제한을 기다리지 않고 즉시 기록합니다. `--engineer-history`는 같은 병합 상태를 JSON Lines로 약 5Hz 누적하므로 입력 CSV만으로는 복원할 수 없었던 랩·순위·갭·연료·ERS·컴파운드·마모·손상·세팅의 시간 변화를 함께 분석할 수 있습니다.

상태 스키마 8의 `race_strategy`에는 현재 스틴트 시작 랩, 대표 랩 수, 랩당 페이스 추세, 제한 타이어와 마모, 예상 결승 마모, 연료 델타, ERS, 피트 윈도·예상 복귀 순위, 최근 8랩과 `traffic_window`가 들어갑니다. `traffic_window`는 현재 위치와 예상 복귀 위치 앞뒤 두 자리의 차량 인덱스, 랩, 갭, 피트 상태·횟수를 담아 전체 22/24대 원본을 5Hz 이력에 반복 저장하지 않고도 AI가 교통을 판단하게 합니다. `radio_revisions`는 플래그, 레이스 컨트롤, 날씨, 피트, 손상, 라이벌과 전략 상태가 AI 응답 중 바뀌었는지 검증합니다. `lifecycle`은 `idle`, `active`, `finished`, `did_not_finish`, `disqualified`, `not_classified`, `ended`, `interrupted`를 구분합니다. Final Classification이 오면 실제 결과 상태와 종료 시각을 기록하고, 결과 패킷 없이 HUD/브리지를 닫으면 `interrupted`와 `bridge_shutdown`을 최종 state/history에 남깁니다. HUD는 종료 토큰을 보낸 뒤 텔레메트리 worker가 마지막 버퍼와 상태를 기록할 때까지 join합니다.

`inputs.csv`와 `corners.csv`의 기존 열 뒤에는 `session_uid`, `session_type`, `session_type_name`이 추가됩니다. 새 UID의 Session 패킷이 아직 도착하지 않은 초기 행은 종류를 빈 값으로 남겨 이전 세션 종류가 새 세션에 섞이지 않게 합니다. 열 폭이 다른 구형 CSV에는 새 행을 append하지 않고 명확한 오류로 중단합니다.

2026 Season Pack의 `CarStatusData`는 다음 ERS 원본을 모두 기록합니다.

- `ers_store_energy`
- `ers_deploy_mode`
- `ers_harvested_this_lap_mguk`
- `ers_harvested_this_lap_mguh`
- `ers_harvest_limit_per_lap` - 2026 전용
- `ers_deployed_this_lap`

`ers_percent`는 게임이 보내는 저장 에너지를 기존 4 MJ 저장소 기준으로 표시하는 파생값입니다. 2026 전략 판단에는 퍼센트 하나만 쓰지 않고 랩별 회수량, 회수 한도, 배포량과 추세를 함께 사용해야 합니다.
