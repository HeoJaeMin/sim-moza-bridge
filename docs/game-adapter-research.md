# ACE / ACR / LU 어댑터 조사

확인일: 2026-05-19

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
| ACR | MOZA는 텔레메트리 지원으로 표시하고, SimHub도 ACR 지원을 표시합니다. 공개 오버레이 도구는 `ACRallyMemReader` 보조 리더를 통해 ACR 지원을 추가했습니다. | 네이티브/보조 리더 어댑터가 필요합니다. F1 방식 UDP나 전체 대시보드 키 범위를 가정하면 안 됩니다. |
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
- 공개 오버레이 도구는 `ACRallyMemReader` 보조 리더를 사용합니다. 이는 로컬 메모리/보조 리더 경로를 가리킵니다.
- 일부 대시보드 스택에서는 초기 지원 범위가 제한적일 수 있습니다.

구현 대상:

```text
ACR 프로세스/보조 메모리 리더
  -> ACR 어댑터
  -> 정규화된 텔레메트리
  -> HUD/로깅/MOZA 출력
```

첫 ACR 어댑터 우선순위:

- 속도
- RPM
- 기어
- 스로틀/브레이크/클러치/조향
- 노출된다면 타이어 온도 또는 트랙션 관련 값
- 가능하다면 랩/스테이지 타이밍
- 가능하다면 스테이지 위치/거리

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

남은 확장 후보:

- 피트레인/플래그 상태
- 트랙 위치 퍼센트와 차량 좌표
- LMU scoring block을 통한 순위/선두 gap 보강

## 추가 검증 작업

Windows PC에서 실제 게임 세션을 실행한 상태로 확인해야 합니다.

1. 게임이 이름 있는 Windows 파일 매핑, 플러그인 파일, 보조 프로세스 중 무엇을 노출하는지 확인
2. 필드 구조에 버전/크기 마커가 있는지 확인
3. Pit House가 ACE/ACR 값을 기존 `v1/gameData/...` 키로 노출하는지 확인
4. LU 키 값이 MOZA Digital Dash 표와 일치하는지 확인
5. REV 라이트와 휠 LED가 RPM 퍼센트, REV 라이트 플래그, 하드웨어별 MOZA 연동 중 무엇으로 구동되는지 확인

## 출처

- MOZA Game Compatibility List: https://support.mozaracing.com/en/support/solutions/articles/70000629729-game-support-list
- MOZA Digital Dash Telemetry Support: https://support.mozaracing.com/en/support/solutions/articles/70000627978-digital-dash-telemetry-support
- Assetto Corsa EVO 0.6 release notes: https://assettocorsa.gg/assetto-corsa-evo-early-access-06-now-available/
- SIM Dashboard Assetto Corsa EVO notes: https://www.stryder-it.de/simdashboard/help/en/For_PC_Gamers/Game_Configuration/Assetto_Corsa_EVO
- Racing Overlay ACR telemetry support notes: https://luizzak.itch.io/racing-overlay/devlog/1321475/assetto-corsa-rally-telemetry-support
- SimHub supported games: https://www.simhubdash.com/supported-games/
- Le Mans Ultimate Telemetry Recording: https://guide.lemansultimate.com/hc/en-gb/articles/14524956311695-Telemetry-Recording
- SIM Dashboard Le Mans Ultimate notes: https://www.stryder-it.de/simdashboard/help/en/For_PC_Gamers/Game_Configuration/Le_Mans_Ultimate
- goLMUSharedMemory API docs: https://pkg.go.dev/github.com/stephenhoran/goLMUSharedMemory
