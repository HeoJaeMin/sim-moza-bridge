# 게임 프로필

브리지는 게임별 입력 방식, 프로토콜 파서, 기본 포트, 지원 가능한 보정 기능을 `--game` 프로필로 구분합니다.

## 현재 프로필

| 프로필 | 게임 | 입력 | 브리지 상태 |
| --- | --- | --- | --- |
| `auto` | 패킷 기반 자동 감지 | UDP 패킷 | F1 25 감지 지원 |
| `f1-25` | F1 25 | UDP 바이너리 패킷 | 지원 |
| `generic-udp` | 외부 UDP 익스포터 | UDP 패킷 | 변경 없이 그대로 전달 |
| `ace` | Assetto Corsa EVO | 공유 메모리 | 어댑터 미구현 |
| `acr` | Assetto Corsa Rally | 공유 메모리 / 보조 리더 계열 | 어댑터 미구현 |
| `lmu` / `lu` | Le Mans Ultimate | 공유 메모리 / 플러그인 기반 텔레메트리 | 어댑터 미구현 |

## 감지 범위

`--game auto`는 들어오는 텔레메트리 패킷을 검사합니다. 실행 중인 프로세스 이름을 훑는 기능이 아닙니다.

현재 지원:

- F1 25 패킷 헤더를 확인하면 `f1-25` 선택
- 알 수 없는 UDP 패킷은 해당 패킷을 원본 UDP로 전달하고, 이후 인식 가능한 패킷을 계속 대기

아직 미지원:

- Windows 프로세스 목록에서 ACE 감지
- Windows 프로세스 목록에서 ACR 감지
- Windows 프로세스 목록에서 LMU/LU 감지
- 공유 메모리 텔레메트리 자동 읽기

프로세스 감지는 나중에 UI 편의 기능으로 추가할 수 있습니다. 다만 프로세스 감지만으로는 충분하지 않습니다. 안전하게 파싱하거나 변환하려면 실제 텔레메트리 프로토콜 어댑터가 필요합니다.

## ACE, ACR, LMU가 다른 이유

F1 25는 문서화된 UDP 프로토콜을 제공하므로 브리지가 게임과 MOZA Pit House 사이에 직접 들어갈 수 있습니다.

```text
F1 25 UDP -> bridge -> MOZA Pit House
```

Assetto Corsa EVO, Assetto Corsa Rally, Le Mans Ultimate는 이 단순 모델에 맞지 않습니다.

Assetto Corsa EVO는 0.6 릴리스 노트에서 공유 메모리 라이브러리 갱신과 공식 MoTeC 지원을 언급합니다. 공개 대시보드 연동 문서도 UDP가 아니라 로컬 텔레메트리로 다룹니다. 따라서 이 브리지는 Windows 공유 메모리 리더와 버전/구조 검증을 갖춘 어댑터가 필요합니다.

Assetto Corsa Rally는 MOZA 게임 호환 목록에서 텔레메트리 지원으로 표시되며, 공개 오버레이 도구에서도 ACR 텔레메트리 리더가 추가되고 있습니다. 다만 MOZA Digital Dash 키 표에는 아직 ACR 전용 컬럼이 없습니다. 그래서 기존 Assetto Corsa 또는 Assetto Corsa Competizione 컬럼만 보고 ACR 키 범위를 단정하면 안 됩니다.

Le Mans Ultimate는 세 게임 중 MOZA 대시 지원 범위가 가장 명확합니다. MOZA Digital Dash Telemetry Support 표에 `Le mans ultimate` 컬럼이 있고, 지원 키가 105개입니다. 일부 서드파티 대시보드는 여전히 rFactor 스타일 공유 메모리 플러그인 경로를 사용할 수 있습니다.

```text
Le Mans Ultimate/Bin64/Plugins/
```

따라서 LMU 어댑터는 현재 공유 메모리 경로와 플러그인 기반 배포를 모두 고려해야 합니다. 외부 익스포터가 명시적으로 UDP를 만들지 않는 한, 일반 게임 UDP 수신기로 구현하면 안 됩니다.

자세한 조사 내용은 [game-adapter-research.md](game-adapter-research.md)에 정리했습니다.

## 로드맵

계획 중인 구조:

```text
게임 원본
  -> 입력 어댑터
  -> 정규화된 텔레메트리 모델
  -> 출력 어댑터
  -> MOZA Pit House 또는 다른 대시보드
```

입력 어댑터:

- `f1-25-udp`: 구현됨
- `generic-udp`: 내용을 해석하지 않고 그대로 전달하는 방식으로 구현됨
- `ace-shared-memory`: 계획
- `acr-shared-memory`: 계획
- `lmu-shared-memory`: 계획

출력 어댑터:

- `moza-udp`: Pit House가 이미 이해하는 패킷에 대해 구현됨
- `web-dashboard`: MOZA가 `v1/gameData/...`로 노출하지 않는 값을 위한 별도 대시보드로 계획

## 지금 사용할 수 있는 방법

F1 25:

```bash
cargo run -- --mode remap --fix-tyre-wear-order
```

외부 익스포터가 이미 호환 UDP를 만드는 경우:

```bash
cargo run -- --game generic-udp --listen 20777 --moza-port 22025
```

ACE, ACR, LMU는 아직 네이티브 어댑터가 없습니다. `--game ace`, `--game acr`, `--game lmu`로 실행하면 UDP 브리지인 것처럼 동작하지 않고, 왜 실행할 수 없는지 명확한 에러를 냅니다.
