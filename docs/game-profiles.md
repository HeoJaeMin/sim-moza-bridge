# 게임 어댑터 메모

현재 실행 CLI는 자동 감지 모드로 고정되어 있습니다. 일반 사용자는 게임 프로필을 선택하지 않습니다.

```bash
cargo run
```

포트를 바꿔야 할 때만 지정합니다.

```bash
cargo run -- --listen 20777 --moza-port 22025
```

브라우저 HUD는 기본 실행 때 `http://127.0.0.1:8765`로 같이 올라오고 기본 브라우저로 열립니다.

## 현재 동작

브리지는 UDP 패킷을 받아 MOZA Pit House로 전달합니다. F1 25 패킷이 감지되면 F1 25용 파서와 보정을 적용하고, 알 수 없는 UDP 패킷은 그대로 전달합니다.

```text
UDP telemetry -> bridge auto-detect -> MOZA Pit House
```

F1 25 CarDamage 패킷은 MOZA가 타이어 웨어를 읽기 쉽도록 F1 24 호환 레이아웃으로 자동 변환합니다.

## 향후 어댑터 후보

Assetto Corsa EVO, Assetto Corsa Rally, Le Mans Ultimate는 F1 25처럼 단순 UDP 입력으로 다루기 어렵습니다.

- Assetto Corsa EVO: 공유 메모리 계열 어댑터 필요
- Assetto Corsa Rally: 네이티브 리더 또는 보조 리더 필요
- Le Mans Ultimate: 네이티브/shared-memory와 플러그인 기반 배포 모두 검토 필요

자세한 조사 내용은 [game-adapter-research.md](game-adapter-research.md)에 정리했습니다.
