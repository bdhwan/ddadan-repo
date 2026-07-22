# ddadan-repo

소상공인용 디지털 사이니지 시스템. NestJS API + Angular 어드민/플레이어 + **Android 셋톱박스 네이티브 플레이어**로 구성된 모노레포.

> 🏬 실제 운영(박스 프로비저닝·메뉴 구성·OTA·배포·진단)은 **[docs/signage-guide.md](docs/signage-guide.md)** 에 절차 위주로 정리되어 있다. 이 README는 개요·구성·API 레퍼런스.

## 구성

- `apps/ddadan-api-server` — NestJS API 서버
  - **SQLite(better-sqlite3, TypeORM)**, `DB_SYNCHRONIZE=true`(마이그레이션 없음)
  - 매장/디바이스/모니터/에셋/화면 + **APK(OTA)/원격명령/텔레메트리/스크린샷** 도메인
  - 정적 에셋 서빙 (`/static/assets/*`), 전역 prefix `/api`, 기본 포트 `7800`
- `apps/ddadan-admin-app` — 관제 UI (Angular)
  - 매장/디바이스 관리, 화면 편집기, **디바이스 버전 관제(플레이어/워치독)·마지막 온라인·최신 APK·전체 OTA 버튼**
  - 원격명령(재부팅/화면on·off/앱업데이트/shell), 스크린샷 갤러리, 자원 텔레메트리
  - 기본 포트 `4200`
- `apps/ddadan-android-app` — **Android 셋톱박스 사이니지 플레이어 (핵심)**
  - `:app`(플레이어) — Jetpack Compose **네이티브 렌더**로 메뉴 보드를 그림. 서버 layout JSON 폴링, 서버 미도달 시 LAN 자동 탐색.
  - `:watchdog` — 별도 패키지. 플레이어 상호 감시·재실행, **OTA(자가/플레이어 갱신)**, 텔레메트리, 원격명령 실행.
  - `:core` — 공용(ServerLocator/RootShell/AppUpdater/DTO/설정).
  - Android 5.1(minSdk 22)·RK3229 박스 대상. root(`su`) 사용. (박스 WebView가 라이브 SPA를 못 그려 네이티브 렌더 채택 — guide 1절 참고)
- `apps/ddadan-client-app` — 브라우저 디스플레이용 HTML 렌더러 (Angular)
  - 박스와 **동일 `ScreenItem` 스키마**를 HTML/CSS로 렌더. `?deviceId=<hardwareId>`로 본인 화면 폴링. 기본 포트 `7300`.
- `services/ddadan-service-pi` — 라즈베리파이 로컬 서비스(하드웨어 ID 인식 + Chromium kiosk).
- `scripts/brotwerk/` — 메뉴 보드 시드(`seed_boards.py`, `menu.json`). 색상 그룹 카드 레이아웃 생성.

## 렌더러 스키마 (박스·브라우저 공유)

`ScreenItem`(`kind`: image/video/text). 텍스트 `textVariant`:
- `menuLine` — `[뱃지] 한글 영문 ····· 가격 (+보조가)`. `textEn`/`priceExtra`/`priceColor`/`badges[]`.
- `groupHeader` — 카테고리 헤더(한글+영문+룰라인+우측라벨).
- `note` — 라운드 틴트 안내박스(✓ + 문구).
- `radius` — 배경 모서리 둥글기(그룹 카드/패널). 색은 hex 및 `rgba()`/`rgb()` 지원.

네이티브 렌더러 `apps/ddadan-android-app/app/.../ui/ScreenItemView.kt`·`ScreenStage.kt`, 웹 렌더러 `apps/ddadan-client-app/src/app/app.{ts,html,scss}` — **두 곳을 함께 수정**.

## 배포 (display-4)

서버는 `ssh display-4`의 `~/ddadan-repo`에서 `docker compose`로 구동(api `:7800`, admin `:4200`, player `:7300`). DB/에셋은 named volume로 유지.

```bash
# 소스 변경 후 배포
rsync -az apps/ddadan-api-server/src/ display-4:~/ddadan-repo/apps/ddadan-api-server/src/
rsync -az apps/ddadan-admin-app/src/  display-4:~/ddadan-repo/apps/ddadan-admin-app/src/
ssh display-4 "cd ~/ddadan-repo && docker compose up -d --build api admin"
```
접속: LAN `http://192.168.150.222:4200`, Tailscale `http://100.96.152.109:4200`.

## Android 앱 빌드 / OTA

```bash
cd apps/ddadan-android-app
./gradlew :app:assembleRelease :watchdog:assembleRelease   # R8 minify + debug 서명
# OTA: 버전 올린 APK를 서버에 업로드 → 박스 워치독이 자가 갱신(설치 후 APK 자동삭제)
curl -X POST http://192.168.150.222:7800/api/apks/upload \
  -F file=@app/build/outputs/apk/release/app-release.apk \
  -F versionCode=NN -F versionName=X.Y -F applicationId=com.ddadan.player
```
상세 절차(프로비저닝·부팅자동실행·설치 트러블슈팅)는 [docs/signage-guide.md](docs/signage-guide.md).

## 로컬 개발

```bash
npm install
npm run dev:api      # NestJS, http://localhost:7800/api  (SQLite 자동 생성)
npm run dev:admin    # 어드민, http://localhost:4200
npm run dev:client   # 브라우저 플레이어
```

## 주요 API

| Method / Path | 설명 |
|---|---|
| `POST /api/devices/check` | 박스 등록 여부 확인 |
| `POST /api/devices` | 디바이스 등록(하드웨어 시리얼=hardwareId) |
| `POST /api/devices/:hardwareId/telemetry` | 워치독 자원/버전 텔레메트리(heartbeat 겸용) |
| `GET /api/player/:hardwareId/screen?slot=0` | 플레이어가 그릴 layout JSON |
| `GET/POST/PATCH /api/screens` | 화면 CRUD (layout JSON) |
| `PATCH /api/monitors/:id/rotation` | 모니터 로테이션 편성(screenIds/interval/fade) |
| `POST /api/apks/upload` · `GET /api/apks/latest?applicationId=` | OTA APK 업로드/최신 조회 |
| `POST /api/devices/:id/commands` | 원격명령 큐잉(reboot/screenOn·Off/updateApp/shell) |
| `GET /api/devices/:hardwareId/commands/pending` · `POST .../ack` | 박스 폴링/완료 보고 |
| `POST /api/devices/:hardwareId/screenshots` · `GET` | 스크린샷 업로드/최근 10장 |
| `POST /api/assets/upload` · `text` | 에셋 업로드 |

## 데이터 모델 요약

`users → stores → devices → monitors`. 각 모니터는 `rotationScreenIds`(로테이션) 또는 `currentScreenId`로 `screens`를 연결.
`screens.layout`은 절대 위치·크기·종류(image/video/text)를 가진 `ScreenItem` 리스트(JSON 컬럼). 디바이스는 하드웨어 시리얼로 식별, 앱버전/워치독버전/마지막온라인/자원 스냅샷을 보관.
