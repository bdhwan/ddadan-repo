# ddadan-relay

로컬 포트를 원격 호스트로 넘기는, 의존성 없는 TCP relay.

## 왜 필요한가

사이니지 서버(display-N)는 Tailscale(`100.x`) 로 어디서든 접근된다. 하지만 **Tailscale 을
못 쓰는 기기** — 스마트TV, 일부 태블릿, 로컬 IP(`192.168.x.x`)만 입력할 수 있는 앱 — 는
`100.x` 도, 다른 네트워크의 LAN IP 도 넣을 수 없다.

그 기기와 **같은 LAN 에 있는 상시 기기 한 대**(집 맥·미니PC·라즈베리파이 등, Tailscale 설치됨)
에 이 relay 를 띄우면, 그 기기의 로컬 IP:포트 로 들어온 연결이 Tailscale 너머 서버로 넘어간다.

```
[집] 스마트TV ──http://192.168.0.246:7300──▶ [집 맥] relay ──Tailscale──▶ [사무실] display-4:7300
```

사이니지 웹(admin/player)은 자기 오리진의 `/api`·`/static` 을 nginx 가 내부 프록시하므로,
포트별 단순 TCP relay 만으로 CORS 문제 없이 그대로 동작한다.

## 사용

Python 3.6+ 만 있으면 설치 없이 실행된다.

```bash
# display-4(Tailscale 100.96.152.109)의 player(7300)+admin(4200)을 이 기기로 노출
./ddadan-relay.py --dest 100.96.152.109 --forward 7300 --forward 4200

# 로컬 포트를 다르게: 로컬 9000 -> 원격 7300
./ddadan-relay.py --dest 100.96.152.109 --forward 9000:7300

# 로컬 전용(외부 노출 안 함)으로 테스트
./ddadan-relay.py --dest 100.96.152.109 --forward 7300 --bind 127.0.0.1
```

| 포트 | 대상 | 브라우저가 직접 부르나 |
|------|------|------------------------|
| 7300 | player (플레이어 화면) | O — relay 필요 |
| 4200 | admin (관리 웹) | O — relay 필요 |
| 7800 | api | X — player/admin nginx 가 내부 프록시. relay 불필요 |

끄기: `pkill -f ddadan-relay.py`

## 부팅 자동실행

`nohup ... &` 는 재부팅 시 사라진다. 상시로 쓸 때만 아래로 감싼다.

### macOS (launchd)

`~/Library/LaunchAgents/com.ddadan.relay.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.ddadan.relay</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/bin/python3</string>
    <string>/path/to/ddadan-relay.py</string>
    <string>--dest</string><string>100.96.152.109</string>
    <string>--forward</string><string>7300</string>
    <string>--forward</string><string>4200</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict></plist>
```

```bash
launchctl load ~/Library/LaunchAgents/com.ddadan.relay.plist
```

### Linux (systemd user unit)

`~/.config/systemd/user/ddadan-relay.service`:

```ini
[Unit]
Description=ddadan TCP relay
After=network-online.target

[Service]
ExecStart=/usr/bin/python3 /path/to/ddadan-relay.py --dest 100.96.152.109 --forward 7300 --forward 4200
Restart=always

[Install]
WantedBy=default.target
```

```bash
systemctl --user enable --now ddadan-relay.service
```

## 주의

- **DHCP 주소.** relay 를 띄운 기기가 DHCP 면 로컬 IP 가 바뀔 수 있다. 상시로 쓸 땐
  공유기에서 MAC 기반 고정 IP 예약을 걸어라.
- **인증 없음.** 순수 포트 포워딩이라 relay 기기가 붙은 LAN 안에서는 누구나 접근한다.
  신뢰된 홈/오피스 LAN 전용. 공용 네트워크에 노출하지 말 것.
- `--dest` 는 보통 대상 서버의 Tailscale IP. LAN IP 를 넣으면 relay 기기와 서버가
  같은 네트워크일 때만 동작한다.
