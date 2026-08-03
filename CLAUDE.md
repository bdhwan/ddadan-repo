# CLAUDE.md — 운영 지침

브로트베르크 디지털 사이니지. **매장 장비는 손댈 수 없다**는 전제로, 모든 운영은 원격으로 한다.

> **구성**: display-4(서버) 1대 + 안드로이드 셋톱박스 3대.
> display-1/2/3(라즈베리파이 디스플레이)는 **회수 후 폐기 예정**이므로 이 문서에서 다루지 않는다.
> 기존 문서에 남아 있는 display-1/2/3 관련 서술은 무시할 것.

## 문서

| 문서 | 언제 보나 |
|---|---|
| [docs/agent-api-guide.md](docs/agent-api-guide.md) | **메뉴 변경** — 가격/문구 수정, 이미지 교체, 보드 배정. API 엔드포인트·스키마·레시피 |
| [docs/signage-guide.md](docs/signage-guide.md) | **배포·운영** — 서버 배포(§6), 앱 OTA(§5), 박스 프로비저닝(§3), 트러블슈팅(§7) |
| [docs/product-design.md](docs/product-design.md) | 제품 기획 배경 |

## 구성

```
[안드로이드 박스 3대] ──5초 폴링──▶ [display-4]  api :7800 / admin :4200
  com.ddadan.player      (Compose 네이티브 렌더)   docker compose (api, admin)
  com.ddadan.watchdog    (감시·OTA·원격명령)        SQLite + 에셋 볼륨
```

| 항목 | 값 |
|---|---|
| 서버 | display-4 — LAN `192.168.150.222`, Tailscale `100.96.152.109` |
| 개발 머신 | rtx-5090-ubuntu — LAN `192.168.150.185`, Tailscale `100.126.172.59` |
| API / Admin | `:7800` / `:4200` (player `:7300` 은 이 구성에서 미사용 — override 로 비활성) |
| 박스 3대 | `U4XCSTB03230301025`(dev 7/mon 8) · `...034`(dev 8/mon 9) · `...031`(dev 9/mon 10) |
| 현재 보드 | 025=BEVERAGE / 034=BAKERY / 031=제품 사진 로테이션 (18초) |
| 매장 네트워크 | 전용 공유기 지참, 사무실과 **동일 SSID/비번** → 매장에서 자동 연결 |

박스는 **하드웨어 시리얼**로 자기를 식별하고, 서버를 못 찾으면 LAN 을 스캔해 자동 발견한다.

## 자주 하는 일

### 메뉴 가격/문구 변경
[agent-api-guide](docs/agent-api-guide.md) 참고. 핵심만:
```bash
API=http://100.96.152.109:7800/api
curl -s "$API/screens/98" > /tmp/s.json      # 1) GET
# 2) layout.items[] 에서 해당 menuLine 의 textSecondary 수정
curl -s -X PATCH "$API/screens/98" -H "Content-Type: application/json" -d @/tmp/patch.json
```
- **PATCH 는 layout 을 통째로 교체**한다. 반드시 GET → 수정 → PATCH.
- 플레이어 1.9+ 는 **재시작 없이 수 초 내 반영**된다.
- 가격 표기는 천단위 소수: 6,000 → `"6.0"`.

### 서버 배포
**Pi 에서 빌드하지 말 것**(30분+). 개발 머신에서 ARM64 크로스 빌드 → 레지스트리 → Pi 가 pull(총 ~2분).
절차는 [signage-guide §6](docs/signage-guide.md). 최초 1회 준비(buildx·레지스트리·insecure 설정)도 거기 있다.

### 안드로이드 앱 업데이트(OTA)
`versionCode` 올리고 release 빌드 → `POST /apks/upload` → 박스가 자동 수신(또는 `updateApp` 명령으로 즉시).
**서명 키는 반드시 동일**해야 한다(`~/.android/debug.keystore`). 절차는 [signage-guide §5](docs/signage-guide.md).

### 상태 확인
```bash
curl -s http://100.96.152.109:7800/api/devices | python3 -m json.tool   # 버전·online·디스크
```
화면 우측 하단 `v10.0/v1.9` 배지(워치독/플레이어)로도 버전을 바로 확인할 수 있다.

## 알아둘 것

- **API 에 인증이 없다.** Tailscale/LAN 등 사설 경로로만 접근하고 인터넷에 노출하지 말 것.
- **백업**: display-4 가 매일 04:00 에 DB+에셋을 구글드라이브(`gdrive:ddadan-backups`)로 올린다.
  30일 초과분은 자동 정리. 수동 실행은 `ssh display-4 '~/.local/bin/ddadan-backup.sh'`.
- **자가복구**: 정전·재부팅 시 docker(`unless-stopped`)와 워치독이 알아서 복구한다. 실측 검증됨.
- **디스크**: 워치독이 85% 초과 시 캐시를 스스로 비운다. admin 기기 목록은 80% 부터 빨간 배지.
- **막혔을 때 마지막 수단은 전원 재인가**다. 박스는 명령을 하나씩 처리하므로 앞의 명령이 끝나지
  않으면 뒤가 전부 막히고, 이때는 `reboot` 명령조차 같은 큐라 닿지 않는다. 서버가 10분 뒤
  자동으로 실패 처리해 큐를 풀지만, 박스 내부 루프가 멎었다면 전원을 껐다 켜야 한다.

## 작업 원칙

- 매장 장비는 **라이브**다. 큰 변경 전에 대상 화면을 `GET` 해서 원본 JSON 을 남겨둘 것.
- 변경 후에는 **실제 화면으로 확인**한다: `POST /devices/{id}/commands {"type":"screenshot"}` →
  `GET /devices/{id}/screenshots?limit=1`. 서버 데이터만 보고 "됐다"고 판단하지 말 것.
- 로테이션에 물린 screen 을 지우면 해당 박스가 그 슬라이드에서 깨진다. 지우기 전에
  `GET /devices` 로 `rotationScreenIds` 를 확인할 것.
