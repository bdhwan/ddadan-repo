# 브로트베르크 디지털 사이니지 — 운영/개발 가이드

셋톱박스(Krizer/RK3229, Android 5.1.1) 기반 매장 메뉴판 사이니지 시스템의 구성·배포·운영 가이드.
**다른 에이전트가 이 문서만으로 동일하게 작업할 수 있도록** 실제 명령/파일 경로/ID를 포함한다.

> 용어: "박스" = 안드로이드 셋톱박스(플레이어 실행). "서버" = display-4의 API 서버. "보드" = 메뉴판 화면.

---

## 1. 아키텍처 개요

```
[박스 android-1/2/3]  ── HTTP 폴링 ──▶  [display-4 서버]
  com.ddadan.player   (/api/player/{serial}/screen)   docker compose:
  com.ddadan.watchdog                                   - api   (:7800, NestJS)
  (네이티브 Compose 렌더)                               - admin (:4200, Angular)
                                                        - player(:7300, client-app)
[브라우저 디스플레이 display-1/2/3] ── 같은 /api ──▶
  client-app(HTML 렌더러)
```

- **박스 플레이어**(`com.ddadan.player`): 네이티브 Compose로 보드를 그린다. 서버에서 `ScreenItem` 레이아웃 JSON을 폴링해 렌더. 서버 미도달 시 LAN 자동 탐색(`/api/health/live` 프로브).
- **워치독**(`com.ddadan.watchdog`): 별도 패키지. 플레이어 감시·재실행, 텔레메트리, 원격명령, **OTA 수행**. 플레이어와 상호 감시(서로 죽으면 재실행).
- **서버**(`apps/ddadan-api-server`, NestJS + better-sqlite3, `DB_SYNCHRONIZE=true`): 디바이스/화면/에셋/APK/명령 관리. display-4에서 docker compose로 구동.
- **admin**(`apps/ddadan-admin-app`, Angular): 관제 UI. `http://100.96.152.109:4200/devices` (또는 LAN `http://192.168.150.222:4200`).
- **client-app**(`apps/ddadan-client-app`, Angular): 브라우저 디스플레이용 HTML 렌더러. **박스는 이걸 안 씀**(WebView 불가 — 3절 참고).

### 왜 박스는 네이티브 렌더인가 (중요)
박스의 System WebView는 Chromium 72지만, **초기 HTML만 한 번 그리고 이후 JS가 바꾸는 DOM을 화면에 반영하지 못한다**(하드웨어=eglChooseConfig 실패, 소프트웨어=동적 repaint 불가). 그래서 라이브 SPA(client-app)를 WebView로 못 띄운다. → **박스는 네이티브 Compose 렌더러**(`ScreenItemView.kt`)를 쓰고, 브라우저 디스플레이만 client-app을 쓴다. 두 렌더러는 **같은 `ScreenItem` 스키마**를 공유한다.

---

## 2. 핵심 좌표 (환경 레퍼런스)

| 항목 | 값 |
|---|---|
| 서버(LAN / Tailscale) | `192.168.150.222` / `100.96.152.109`, api `:7800`, admin `:4200`, player `:7300` |
| 서버 배포 위치 | `ssh display-4` → `~/ddadan-repo`, `docker compose` |
| 박스 식별자 | 하드웨어 시리얼 `ro.serialno` (예: `U4XCSTB03230301025`) |
| android-1/2/3 시리얼 | `...031` / `...025` / `...034` (device id 9 / 7 / 8) |
| store | Default (`storeId=1`) |
| 앱 패키지 | 플레이어 `com.ddadan.player`, 워치독 `com.ddadan.watchdog` |
| Krizer 패키지 | `kr.co.gogobox.android`, `kr.co.gogobox.monitor` |
| 스톡 런처 | `com.android.launcher3` |
| 메뉴 시드 | `scripts/brotwerk/seed_boards.py`, `menu.json` |
| APK 산출물 | `apps/ddadan-android-app/app/build/outputs/apk/release/app-release.apk`, `watchdog/build/outputs/apk/release/watchdog-release.apk` |

---

## 3. 새 박스 프로비저닝 (신규 설치 절차)

전제: 박스가 개발자옵션 USB디버깅 ON + "Connect to PC"(ADB 모드). root(`su`) 사용 가능.

```bash
# 0) 연결 확인 (안 잡히면 adb kill-server && adb start-server)
adb devices -l                       # 시리얼 확인, S에 대입
S=U4XCSTB0323030103X

# 1) 크라이저 비활성화 (재부팅해도 유지됨)
adb -s $S shell "su -c 'pm disable kr.co.gogobox.android; pm disable kr.co.gogobox.monitor'"

# 2) dexopt 안전장치 + 앱 설치 (플레이어 dex가 커서 dex2oat SIGSEGV 방지)
adb -s $S shell "su -c 'setprop dalvik.vm.dex2oat-filter interpret-only'"
adb -s $S install -r watchdog/build/outputs/apk/release/watchdog-release.apk
adb -s $S install -r app/build/outputs/apk/release/app-release.apk
#   ⚠ 실행 중이면 install이 실패처럼 보일 수 있음 → force-stop 후 재시도:
#   adb -s $S shell "su -c 'am force-stop com.ddadan.player'"; adb -s $S install -r ...

# 3) 서버에 디바이스 등록 (하드웨어 시리얼 = hardwareId)
API=http://192.168.150.222:7800/api
curl -s -X POST "$API/devices" -H "Content-Type: application/json" \
  -d '{"hardwareId":"'$S'","storeId":1,"name":"android-N","monitors":[{"slot":0,"resolutionW":1920,"resolutionH":1080}]}'
#   → 반환된 device id / monitor id 확인:
curl -s "$API/devices" | python3 -c "import sys,json;[print(d['id'],d['hardwareId'],[m['id'] for m in d['monitors']]) for d in json.load(sys.stdin)]"

# 4) 보드 배정 (4절 참고) — 모니터에 화면 로테이션 지정

# 5) 부팅 자동 실행: 스톡 런처를 꺼서 우리 플레이어를 유일한 홈으로
adb -s $S shell "su -c 'pm disable com.android.launcher3'"
#   → 재부팅하면 플레이어가 곧바로 뜬다(+플레이어/워치독 BootReceiver도 자동 시작).

# 6) 실행
adb -s $S shell "am start -n com.ddadan.player/.MainActivity"
```

### 등록 정보
박스는 **하드웨어 시리얼(`ro.serialno`)**로 자신을 식별한다(`DeviceIdentity.hardwareId`, 실패 시 `Settings.Secure.ANDROID_ID`). 미등록이면 화면에 "브로트베르크 등록이 필요합니다 · 등록 코드: {시리얼}"이 뜬다. 모니터 해상도도 박스가 보고해 자동 동기화된다.

### 네트워크 필수
박스는 서버(`192.168.150.x`)와 같은 LAN에 있어야 한다. **WiFi에 저장된 네트워크가 없으면 IP를 못 받아 서버에 못 붙는다.** 증상: 탐색 화면에 "IP 없음 — 네트워크 미연결". 박스 설정에서 WiFi 연결 필요(비밀번호는 사람이 입력). 이더넷도 가능.

---

## 4. 메뉴 보드 구성 방법

메뉴 데이터는 `scripts/brotwerk/menu.json`, 레이아웃은 `seed_boards.py`가 생성해 서버에 POST한다.

### 4.1 데이터 (`menu.json`)
```json
{
  "bread":    [{"name":"소금빵","price":2800,"en":"Salt Bread","group":"bread","badges":[{"text":"BEST","variant":"best"}]}, ...],
  "beverage": [{"name":"아메리카노","price":3000,"en":"Americano","group":"espresso","badges":[...],"extra":"+1,000"}, ...],
  "beverage_addons": [{"name":"샷 추가","en":"Extra Shot","price":"+500"}, ...]
}
```
- `group`: 베이커리 `bread`/`sweet`/`sandwich`, 음료 `espresso`/`coldbrew`/`tea`.
- `badges[].variant`: `best`(파란원)/`rec`(초록원, "추천")/`info`(회색 이탤릭, "ICED Only")/`warn`(초록박스, "DECAF").
- `extra`: 이중가격 보조가(EXTRA SIZE). `price`가 숫자면 원, 문자열이면 그대로.

### 4.2 레이아웃 (`seed_boards.py`)
- `beverage_layout(...)`: 사진 우측 + 3색 그룹 카드(에스프레소/콜드브루·에이드/티·주스) + 안내박스 + 추가 카드.
- `bakery_layout(...)`: 사진 좌측 + 3색 그룹 카드(빵/단과자·잼/샌드위치) + 안내박스.
- 공통 스타일: **색상 라운드 카드**(그룹 구분), 컬러 액센트 바, 큰 폰트, **가격 천단위 소수 표기**(4,000→`4.0`, +1,000→`+1.0` — `_kprice`/`_kextra`), 그룹 다수(2개↑)가 사이즈업일 때만 헤더 `EXTRA SIZE`.
- 색/틴트: `BEV`/`BAKERY` dict = `(한글, 영문, 헤더색, 안내박스틴트, 카드틴트)`. **박스 디스플레이는 저알파를 워시아웃하므로 카드 틴트는 0.13~0.17, 안내박스 0.30 정도로 진하게.**

### 4.3 시드 실행 (보드 → 모니터 배정)
```bash
cd scripts/brotwerk && python3 - <<'PY'
import sys, json; sys.path.insert(0,"."); import seed_boards as sb
API="http://192.168.150.222:7800/api"
menu=json.load(open("menu.json"))
# 음료:
bev=menu["beverage"]
groups={k:[m for m in bev if m.get("group")==k] for k in ("espresso","coldbrew","tea")}
ids=[]
for a in [57,58,59]:                      # 음료 이미지 asset id (파일명 vl=음료, bl=베이커리)
    p={"name":"BEVERAGE", **sb.beverage_layout("BEVERAGE","음료",groups,menu["beverage_addons"],a)}
    ids.append(sb.api(API,"POST","/screens",p)["id"])
sb.api(API,"PATCH","/monitors/{MON}/rotation",{"screenIds":ids,"intervalMs":18000,"fadeMs":900})
# 베이커리:
bread=menu["bread"]
bg={k:[m for m in bread if m.get("group")==k] for k in ("bread","sweet","sandwich")}
ids=[sb.api(API,"POST","/screens",{"name":"BAKERY", **sb.bakery_layout(bg,"매일 매장에서 갓 구워냅니다",a)})["id"] for a in [54,55,56]]
sb.api(API,"PATCH","/monitors/{MON}/rotation",{"screenIds":ids,"intervalMs":18000,"fadeMs":900})
PY
```
- 에셋 id는 `GET /api/assets`로 확인(`originalName` 참고, `vl`=음료·`bl`=베이커리 사진). `{MON}`은 대상 박스의 monitor id.
- `intervalMs`=슬라이드 표시 시간(ms), `fadeMs`=크로스페이드. 로테이션 시간 변경은 이 `PATCH`만 다시 호출.

### 4.4 렌더러 스키마 (`ScreenItem`) — 서버/네이티브/웹 공유
`kind`: `image`/`video`/`text`. 텍스트 변형 `textVariant`:
- `menuLine`: `[뱃지] 한글(text) 영문(textEn) ····· 가격(textSecondary) 보조가(priceExtra)`. `priceColor`로 가격색.
- `groupHeader`: 한글(text) + 영문(textEn) + 룰라인 + 우측라벨(textSecondary).
- `note`: 라운드 틴트 박스 + ✓ + 문구. `background`=틴트, `color`=글자.
- `radius`: 배경 모서리 둥글기(px) — 그룹 카드/패널.
- 색은 hex(`#rrggbb`) 또는 **`rgba()`/`rgb()`** 지원(네이티브 `parseColorOrNull`이 파싱). ⚠ rgba는 반드시 이 파서를 거쳐야 함(과거 hex만 지원해 카드 틴트가 투명하게 나온 버그 있었음).

렌더러 파일: 네이티브 `apps/ddadan-android-app/app/.../ui/ScreenItemView.kt`·`ScreenStage.kt`, 웹 `apps/ddadan-client-app/src/app/app.{ts,html,scss}`. **두 곳을 항상 함께 수정**.

---

## 5. 안드로이드 OTA (앱 업데이트) 방식

### 5.1 동작 원리
- 워치독이 주기적으로(`otaLoop` ~10분, 또는 admin `updateApp` 명령 시 즉시) `GET /api/apks/latest?applicationId=`를 확인.
- 서버 최신 `versionCode` > 설치 버전이면 **다운로드 → `su pm install -r` 설치**. 대상 앱은 설치 중 죽지만 **상호 감시로 상대 앱이 재실행**(플레이어↔워치독). 재부팅/setsid 불필요.
- **설치 후 다운로드 캐시 + 스테이징 APK를 반드시 삭제**(박스 저장공간 부족 — 필수). 다음 사이클 시작 시 잔여도 청소.
- OTA 진행 중 플레이어 **좌상단에 "APK 업데이트 중 NN%"** 오버레이 표시(워치독→플레이어 브로드캐스트 `com.ddadan.OTA_PROGRESS`, `AppUpdater`→`OtaProgress`).
- 핵심 파일: `apps/ddadan-android-app/core/.../AppUpdater.kt`, `OtaBroadcast.kt`, `WatchdogService.kt`.

### 5.2 새 버전 릴리스 절차
```bash
cd apps/ddadan-android-app
# 1) 버전 올림 (app/build.gradle.kts): versionCode +1, versionName
# 2) 빌드 (R8 minify + debug 키스토어 서명 — OTA는 같은 키여야 pm install -r 호환)
./gradlew :app:assembleRelease           # 렌더러/플레이어 변경 시
./gradlew :watchdog:assembleRelease      # 워치독 변경 시 (둘 다 :core 의존)
# 3) 서버에 업로드 → 이후 모든 박스가 OTA로 자가 업데이트
API=http://192.168.150.222:7800/api
curl -s -X POST "$API/apks/upload" \
  -F "file=@app/build/outputs/apk/release/app-release.apk" \
  -F "versionCode=22" -F "versionName=1.6" -F "applicationId=com.ddadan.player"
# 4) 즉시 적용하려면 특정 박스에 updateApp 명령 큐잉(워치독 commandLoop ~10s가 처리):
curl -s -X POST "$API/devices/{DEVICE_ID}/commands" -H "Content-Type: application/json" -d '{"type":"updateApp"}'
```
- admin `http://.../devices`:
  - 상단에 **서버 최신 APK 버전**(OTA 페이로드) + **"안드로이드 전체 업데이트" 버튼**(전체 디바이스에 `updateApp` 명령 큐잉).
  - 각 행에 **설치 버전을 플레이어/워치독 분리 표시**(워치독 텔레메트리가 `appVersion`=플레이어, `watchdogVersion`=워치독 자신을 각각 보고) + **마지막 온라인**.
- 개발 중 즉시 반영은 `adb install -r`(위 프로비저닝 참고, force-stop 후). **OTA 경로 자체를 검증**하려면 버전 올려 업로드 후 워치독이 받아가는지 확인.
- 현재 버전(참고): 플레이어 `1.7 (23)`, 워치독 `8.0 (8)`. OTA는 플레이어·워치독 각각의 `apks/latest?applicationId=`로 독립 갱신.
- 워치독 주기(참고): 플레이어 감시 `2s` · 원격명령 폴링 `10s` · 텔레메트리(하트비트) `120s` · OTA 확인 `10분`. 서버 오프라인 판정은 `360s`(하트비트의 3배).

### 5.3 서명 키 (⚠ OTA 생명줄)
릴리스 APK도 **안드로이드 디버그 키스토어**로 서명한다 — 이미 배포된 앱을 `pm install -r`로 갱신하려면 **서명 키가 반드시 같아야** 하기 때문(`app/build.gradle.kts`의 `signingConfigs.release`).
```
키스토어: ~/.android/debug.keystore   alias: androiddebugkey   비번: android / android
현재 서명 지문 SHA-256: 7623a8459165d59e8c2686a2cdf0245271a08b9f0b2c6c85b2d251302877ac6b
```
- **다른 머신에서 빌드하면 키가 달라져 OTA가 거부된다.** 반드시 같은 `debug.keystore`를 쓸 것(필요 시 이 파일을 복사해 사용). 파일 분실 시 박스 전체 수동 재설치 필요.
- OTA가 안 먹으면 먼저 서명부터 확인: `apksigner verify --print-certs <apk>` 결과를 위 지문과 대조.
- 디버그 키는 공개 기본 키라 출처 보증이 안 되고 Play 배포도 불가 — 사내 폐쇄망 전용이라는 전제로 유지 중.

### 5.4 주의
- 큰 Compose dex → `INSTALL_FAILED_DEXOPT`(dex2oat SIGSEGV). 완화: `setprop dalvik.vm.dex2oat-filter interpret-only`. 근본 해결: R8 minify(이미 적용, ~2.5MB).
- 저장공간 부족(`No space left`) → R8 minify로 해결됨. OTA 후 APK 삭제 필수.

---

## 6. 서버/Admin 배포

display-4에서 docker compose로 구동. **빌드는 개발 머신에서 하고 Pi 는 받아 쓰기만 한다.**

> ⚠ `docker compose up --build` 를 Pi 에서 돌리지 말 것. Pi 가 소스를 직접 컴파일해
> **30분 이상** 걸린다(같은 빌드가 개발 머신에서 ARM64 크로스 빌드로 **2분**). 예전 문서의
> `rsync` + `--build` 절차가 이것이었다.

### 6.1 최초 1회 준비 (개발 머신)
```bash
sudo apt-get install -y docker-buildx qemu-user-static binfmt-support   # ARM64 크로스 빌드
docker run --privileged --rm tonistiigi/binfmt --install arm64

# 로컬 레지스트리(항상 켜둠) — Pi 가 여기서 이미지를 받아간다
docker run -d --name ddadan-registry -p 5000:5000 --restart unless-stopped registry:2

# buildx 빌더: 평문 HTTP 레지스트리 허용 (없으면 push 가 HTTPS 를 요구하며 실패)
cat > /tmp/buildkitd.toml <<EOF
[registry."192.168.150.185:5000"]
  http = true
  insecure = true
EOF
docker buildx create --name ddadan-builder --use --config /tmp/buildkitd.toml
```

### 6.2 최초 1회 준비 (Pi, sudo 필요)
평문 레지스트리를 신뢰하게 한다. **LAN 과 Tailscale 주소를 모두** 넣어야 사무실 밖에서도 배포된다.
```bash
ssh -t display-4 'echo "{\"insecure-registries\":[\"192.168.150.185:5000\",\"100.126.172.59:5000\"]}" \
  | sudo tee /etc/docker/daemon.json && sudo systemctl restart docker'
```

### 6.3 평소 배포 (소스 변경 후)
```bash
cd ~/git/ddadan-repo                      # 개발 머신
REG=192.168.150.185:5000                  # 사무실 밖이면 100.126.172.59:5000

# 1) ARM64 크로스 빌드 → 레지스트리로 바로 push (~2분)
docker buildx build --platform linux/arm64 \
  -f apps/ddadan-api-server/Dockerfile -t $REG/ddadan-repo-api:latest --push .

# 2) Pi 가 받아서 컨테이너 교체 (빌드 없음)
ssh display-4 "docker pull $REG/ddadan-repo-api:latest \
  && docker tag $REG/ddadan-repo-api:latest ddadan-repo-api:latest \
  && cd ~/ddadan-repo && docker compose up -d --no-build --force-recreate api"
```
admin 은 `apps/ddadan-admin-app/Dockerfile` + `ddadan-repo-admin` 으로 같은 절차.

- **`docker save | ssh docker load` 는 쓰지 말 것.** 특정 레이어에서 진행이 멈춘 채 매달린다
  (Pi 부하는 0 인데 끝나지 않음). 레지스트리 경유가 확실하다.
- 배포 후 검증: `curl -s -o /dev/null -w "%{http_code}" http://100.96.152.109:7800/api/health/live` → 200.
- DB/에셋은 named volume이라 재빌드에도 유지. 화면 레이아웃 새 필드는 JSON `layout` 안이라 마이그레이션 불필요.
- ⚠ `ValidationPipe(whitelist:true)` → **새 스키마 필드는 `screen.dto.ts`에 추가해야 통과**(안 그러면 서버가 벗겨냄). 추가 후 반드시 서버 재배포.
- api 재시작 시 박스가 잠깐 서버를 놓쳐 탐색 화면이 뜰 수 있음(재발견되면 복구).

---

## 7. 진단 / 트러블슈팅

- **서버 탐색 화면(전체)**: **보여줄 콘텐츠가 아직 없을 때만**(첫 부팅 등) 전체 화면으로 표시. **기기 진단 정보**(기기 ID, IP 주소[없으면 "없음 — 네트워크 미연결" 빨강], WiFi 상태/SSID, 게이트웨이, 스캔 대상 서브넷, 서버 주소, 앱 버전)를 함께 보여줌 → 원인 즉시 파악. (`PlayerScreen.DiscoveryOverlay` + `util/NetworkDiag.kt`)
- **서버 다운 시 동작**: 앱은 안 꺼지고 **마지막 메뉴가 화면에 그대로 유지**된다. 덮지 않고 **좌상단에 작은 "서버 재연결 중" 배지**만 표시(`ReconnectingBadge`). 서버 복구 시 조용히 재개.
  - 폴링 실패가 **연속 10회(`MAX_CONSECUTIVE_FAILURES`)** 쌓여야 LAN 전체 스캔으로 넘어간다(짧은 블립엔 스캔 안 함).
  - 스캔에도 못 찾으면 **30초(`RETRY_DELAY_SEC`) 후 재탐색**(과거 3분에서 단축).
- **부팅 시**: 네트워크가 붙을 때까지 대기(`awaitingNetwork`, "네트워크 연결 대기 중...")하고, **캐시된 보드를 먼저 표시**한 뒤 서버와 동기화한다.
- **스크린샷**: 주기 촬영이 아니라 **온디맨드** — admin에서 `screenshot` 원격명령을 보내면 그때 캡처·업로드(서버는 기기당 최근 10장 유지).
- **`adb install` "Success" 없이 조용히 실패**: 앱 실행 중 `pm install -r` 간섭. `am force-stop` 후 재설치.
- **adb 기기 안 잡힘**: `adb kill-server && adb start-server`, 박스 "Connect to PC"/USB디버깅 확인.
- **원격 명령이 전부 `pending` 에서 안 넘어감 (박스는 online)**: 박스는 명령을 **순서대로 하나씩**
  처리하므로 앞의 명령이 끝나지 않으면 뒤가 전부 막힌다. 텔레메트리는 별도 루프라 계속 올라와
  **online 으로 보이는 게 함정**. 증상 확인은 `GET /devices/{id}/commands` 로 pending 이 쌓이는지 보면 된다.
  - 서버가 **10분(`COMMAND_TIMEOUT_SECONDS`) 뒤 자동으로 failed 처리**해 큐를 푼다(1분 주기 스윕).
    큐가 풀리면 같은 작업을 다시 큐잉해 재시도할 수 있다.
  - 그래도 박스 쪽 루프 자체가 멎었다면 **원격 수단이 없다**(`reboot` 명령도 같은 큐를 탄다).
    **전원 재인가만이 복구 방법.** 켜면 워치독이 새로 시작하며 밀린 OTA 까지 자동으로 받아간다.
- **OTA 진행률이 되돌아감(예: 68% → 3%)**: `otaLoop`(10분 주기)과 `commandLoop`(updateApp 명령)이
  같은 APK 를 같은 캐시 파일에 **동시에** 내려받던 문제. 겹쳐 쓴 APK 는 설치에 실패하고 그 명령이
  끝나지 않아 위의 "명령 전부 pending" 으로 이어졌다. 워치독 v10.0 에서 뮤텍스로 차단하고,
  updateApp 은 별도 코루틴으로 돌려 명령 루프를 붙잡지 않게 했다.
- **화면 우측 하단 `v10.0/v1.9` 배지**(워치독/플레이어): 어느 박스가 구버전인지 화면만 보고 판단.
  배지가 안 보이면 플레이어가 1.9 미만이라는 뜻.

---

## 8. 구현 이력 (요약)

**렌더링/메뉴**
- `ScreenItem` 스키마 확장: `textEn`/`priceExtra`/`priceColor`/`badges[]`/`radius`, `textVariant`에 `groupHeader`·`note`.
- 네이티브 렌더러(`ScreenItemView.kt`/`ScreenStage.kt`): 뱃지·영문병기·이중가격·그룹헤더·안내박스·라운드카드, **`parseColorOrNull` rgba 지원**, 크로스페이드 튕김 수정.
- 시드 재작성: 색상 그룹 카드 + 천단위 표기 + `beverage_layout`/`bakery_layout`.
- 브랜딩: 화면 표시 "DDADAN" → "브로트베르크".

**운영/관제**
- OTA 진행 오버레이(좌상단), 워치독 텔레메트리가 플레이어/워치독 버전 분리 보고.
- admin `/devices`: 플레이어/워치독 버전 분리 + 마지막 온라인 + 상단 최신 APK + 전체 OTA 버튼.
- 부팅 자동실행(스톡 런처 비활성화).

**연결 안정화 (이후 반영)**
- 서버 끊겨도 **보드 유지 + 좌상단 재연결 배지**(전체 탐색 화면은 콘텐츠 없을 때만).
- 폴링 **연속 10회 실패** 후에만 LAN 스캔, 재탐색 대기 **3분 → 30초**.
- 부팅 시 **네트워크 대기 + 캐시 보드 우선 표시**.
- 스크린샷 **주기 → 온디맨드**(`screenshot` 명령), 하트비트 `120s`/오프라인 판정 `360s`.

> 원격에서 **메뉴를 API로 편집**하는 방법(LLM 에이전트용)은 별도 문서 **[agent-api-guide.md](agent-api-guide.md)** 참고.
