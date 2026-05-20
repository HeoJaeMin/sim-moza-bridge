# 게임 어댑터 메모

기본 실행은 F1 25 UDP 자동 감지 모드로 시작합니다.

```bash
cargo run
```

LMU/ACE처럼 UDP 입력이 아닌 게임은 adapter 프로필을 지정합니다.

```bash
cargo run -- --game lmu
cargo run -- --game ace
```

Windows에서 기본 `cargo run`은 F1 UDP, `LMU_Data`, `Local\acevo_pmf_physics`를 함께 감시합니다. F1 25 UDP가 들어오면 F1 경로를 우선하고, F1 입력이 멈추면 LMU/ACE 공유 메모리 경로를 다시 읽습니다.

`--game lmu`와 `--game ace`는 명시적으로 해당 adapter만 고정 실행할 때 사용합니다.

포트를 바꿔야 할 때만 지정합니다. 포트 옵션은 F1/UDP 경로에만 의미가 있습니다.

```bash
cargo run -- --listen 20777 --moza-port 22025
```

브라우저 HUD는 기본 실행 때 `http://127.0.0.1:8765`로 같이 올라오고 기본 브라우저로 열립니다.

## 현재 동작

기본 브리지는 UDP 패킷을 받아 MOZA Pit House로 전달합니다. F1 25 패킷이 감지되면 F1 25용 파서와 보정을 적용하고, 알 수 없는 UDP 패킷은 그대로 전달합니다.

```text
UDP telemetry -> bridge auto-detect -> MOZA Pit House
```

F1 25 CarDamage 패킷은 MOZA가 타이어 웨어를 읽기 쉽도록 F1 24 호환 레이아웃으로 자동 변환합니다.

## F1과 LMU 활성화 차이

F1 25는 게임 안에서 UDP Telemetry를 켜고 IP/포트/포맷을 맞추면 브리지가 바로 받을 수 있습니다.

Le Mans Ultimate는 F1처럼 `UDP Telemetry On`과 포트 설정만으로 이 브리지에 직접 들어오는 구조로 취급하면 안 됩니다. `cargo run -- --game lmu`는 LMU shared-memory adapter를 구동하고 `LMU_Data`에서 실시간 HUD 텔레메트리를 읽습니다.

Assetto Corsa EVO도 F1 UDP 경로가 아닙니다. `cargo run -- --game ace`는 `Local\acevo_pmf_physics` 공유 메모리에서 기본 주행 텔레메트리를 읽습니다.

## 어댑터 상태

Assetto Corsa EVO, Assetto Corsa Rally, Le Mans Ultimate는 F1 25처럼 단순 UDP 입력으로 다루기 어렵습니다.

- Assetto Corsa EVO: `Local\acevo_pmf_physics` adapter 구현
- Assetto Corsa Rally: 네이티브 리더 또는 보조 리더 필요
- Le Mans Ultimate: `LMU_Data` adapter 구현

자세한 조사 내용은 [game-adapter-research.md](game-adapter-research.md)에 정리했습니다.
