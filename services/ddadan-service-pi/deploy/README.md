# DDADAN Pi 키오스크 배포 (systemd user 서비스 + 5분 watchdog)

라즈베리파이(Debian 13 / Wayland·labwc)에서 부팅 시 Node 서비스가 자동 실행되어
디바이스 등록여부를 확인하고 chromium을 전체화면(kiosk)으로 띄웁니다. 5분마다
watchdog cron이 실행여부를 점검해 끊기면 재실행합니다.

## 동작 구조

```
부팅 → labwc 세션 시작 → ~/.config/labwc/autostart
         └ systemctl --user restart ddadan-pi.service
systemd --user (Restart=always) → node dist/index.js
         ├ /devices/check → 등록되면 player(7300), 미등록이면 admin(4200/register)
         └ kiosk.ts → deploy/kiosk-launch.sh <url>
                        └ wlr-randr로 모니터 대기 → chromium --kiosk --ozone-platform=wayland
cron */5 → deploy/ddadan-watchdog.sh : 서비스 active? & chromium 떠있음? 아니면 재시작
```

| 파일 | 역할 |
|------|------|
| `kiosk-launch.sh` | Wayland/labwc 런처. 모니터 연결 대기(wlr-randr), 블랭킹 해제(wlopm), 해상도 감지 후 chromium kiosk 실행. 모니터 분리/chromium 종료 시 정리하고 종료 → 서비스가 재실행. X11 폴백 포함. |
| `ddadan-pi.service` | systemd **user** 유닛 템플릿. `Restart=always`, `WAYLAND_DISPLAY=wayland-0`. |
| `ddadan-watchdog.sh` | 5분 watchdog. `systemctl --user` 서비스 + chromium 프로세스 점검 후 재시작. |
| `install.sh` | 한방 설치: Node 24 설치 → 빌드 → .env → user 서비스 등록 → autostart 치환 → cron 등록. |
| `uninstall.sh` | 서비스/크론 제거, autostart 백업 복원. |

## 설치

라즈베리파이에서 **데스크톱 사용자로** (root 아님) 실행:

```bash
cd ~/ddadan-repo/services/ddadan-service-pi
./deploy/install.sh
# 서버(display-1) IP가 다르면:
DDADAN_SERVER_IP=100.87.216.19 ./deploy/install.sh
```

install.sh가 수행하는 것:
- Node 24 미설치 시 NodeSource로 설치 (sudo)
- `npm ci && npm run build`
- `.env`에 API/player/admin 주소와 `DDADAN_LAUNCH_KIOSK=1`, 런처 경로 설정
- `~/.config/systemd/user/ddadan-pi.service` 설치 + enable + `enable-linger`
- 기존 `~/.config/labwc/autostart`의 인라인 chromium 루프를 **서비스 시작 호출로 치환** (원본은 `autostart.ddadan-bak`로 백업) — chromium 이중 실행 방지
- 사용자 crontab에 `*/5` watchdog 등록

## 디바이스 등록

Node 서비스는 미등록 기기를 admin 등록화면으로 보냅니다. 미리 등록하려면:

```bash
curl -X POST http://<server-ip>:7800/api/devices \
  -H 'Content-Type: application/json' \
  -d '{"hardwareId":"display-1","storeId":1,"name":"display-1"}'
```

(`hardwareId`는 각 기기의 `hostname`과 일치해야 함.)

## 확인 / 디버깅

```bash
systemctl --user status ddadan-pi          # active(running)
journalctl --user -u ddadan-pi -f          # 서비스 로그 (kiosk launching ... )
tail -f ~/.local/state/ddadan-watchdog.log # watchdog 로그
crontab -l                                  # 5분 cron 확인
```

복구 테스트:

```bash
pkill -f -- --kiosk                         # chromium 강제 종료 → 수초 내 자동 재실행
~/ddadan-repo/services/ddadan-service-pi/deploy/ddadan-watchdog.sh  # watchdog 수동 실행
sudo reboot                                 # 재부팅 후 자동 전체화면 확인
```

## 참고

- 화면 절전은 Wayland에서 `wlopm --on '*'`로 출력 전원을 유지합니다. labwc에 swayidle
  타임아웃이 설정돼 있다면 별도 해제가 필요할 수 있습니다.
- 롤백: `./deploy/uninstall.sh` 실행 후 재로그인/재부팅하면 기존 labwc autostart가 복원됩니다.
