# 사이니지 메뉴 원격 편집 — LLM 에이전트용 API 가이드

이 문서 하나로 LLM 에이전트가 **매장 메뉴판을 원격에서 안전하게 변경**할 수 있게 하는 것이 목표다.
실제 엔드포인트·스키마·검증된 예시를 담는다. 운영/배포 전반은 [signage-guide.md](./signage-guide.md) 참고.

> 대상 독자: API를 직접 호출하는 에이전트(또는 사람). "매장에 가서 손댈 수 없다"가 전제 —
> 모든 메뉴 변경은 이 API만으로 끝난다.

---

## 0. 접속 (base URL & 인증)

| 경로 | base URL |
|---|---|
| Tailscale (권장, 어디서나) | `http://100.96.152.109:7800/api` |
| 매장/사무실 LAN | `http://192.168.150.222:7800/api` |
| 집에서 relay 경유 | `http://192.168.0.246:7800/api` (relay 실행 중일 때) |

- **인증 없음.** API에 인증 가드가 없다 — 네트워크에 닿으면 누구나 호출한다. 따라서
  **반드시 Tailscale/LAN/relay 같은 사설 경로로만** 접근하고, 인터넷에 노출하지 말 것.
- 서버는 display-4에서 docker로 구동(`api` 컨테이너, better-sqlite3). `store`는 `Default`(`storeId=1`).
- 상태 확인: `GET /api/health/live` → `200`.

### 렌더링 원리 (변경 결과가 화면에 어떻게 반영되나)
- 화면(`screen`)은 좌표 기반 레이아웃(`ScreenItem` 배열)이다. **디자인 좌표계는 항상 1920×1080** —
  실제 패널 해상도와 무관하게 이 안에서 x/y/width/height를 잡으면 렌더러가 비율로 스케일한다.
- 안드로이드 박스와 브라우저 디스플레이가 **같은 스키마**를 쓴다. 한 번 바꾸면 양쪽에 동일 반영.
- 박스는 서버를 5초마다 폴링한다. **PATCH 하면 수 초 내 화면에 반영**된다(수동 배포 불필요).
  - 단 **플레이어 1.9 이상**에서만. 그 이전 버전은 로테이션 갱신 여부를 아이템 `id` 로만 판단해서,
    가격·문구처럼 id 가 그대로인 편집은 화면에 반영되지 않았다(플레이어 재시작이 필요했다).
    화면 우측 하단 `v10.0/v1.9` 배지로 버전을 확인할 수 있다.
- 편집이 화면에 안 나타나면 [signage-guide 7절](./signage-guide.md)의 "원격 명령이 전부 pending" 항목을 볼 것.

---

## 1. 메뉴 변경의 3가지 작업

| 작업 | 난이도 | 요약 |
|---|---|---|
| A. 가격·이름·뱃지 변경, 항목 추가/삭제 | 쉬움 | 기존 screen의 `layout.items` 편집 → `PATCH /screens/:id` |
| B. 이미지 교체 | 쉬움 | `POST /assets/upload` → item의 `assetId` 교체 → PATCH |
| C. 새 보드 디자인 | 중간 | `scripts/brotwerk/seed_boards.py`의 레이아웃 생성기 사용 권장 |

**황금률**: 새 보드를 맨손으로 좌표 찍지 말고, **기존 화면을 GET → 복제·수정 → PATCH/POST** 하라.
정교한 레이아웃(그룹 카드·색·간격)은 `seed_boards.py`가 이미 만들어 두었다.

---

## 2. 엔드포인트 레퍼런스 (메뉴 편집에 쓰는 것만)

### 화면(보드)
```
GET    /api/screens              # 전체 목록(요약)
GET    /api/screens/:id          # 단일 화면 전체 layout
POST   /api/screens              # 새 화면 (body: CreateScreenDto)
PATCH  /api/screens/:id          # 수정 (body: UpdateScreenDto — layout 통째로 교체)
DELETE /api/screens/:id          # 삭제
```
⚠ `PATCH`의 `layout`은 **부분 병합이 아니라 통째 교체**다. 반드시 GET 한 layout을 수정해서 보낼 것.

### 에셋(이미지)
```
GET    /api/assets                       # 목록 (originalName으로 식별)
POST   /api/assets/upload                # multipart: file=@image.jpg [, storeId]
PATCH  /api/assets/:id                   # 메타 수정
DELETE /api/assets/:id
```
- 업로드 응답의 `id`를 item의 `assetId`로, `url`(예: `/static/assets/...`)은 렌더러가 자동 조합.
- **⚠ 사용 중인 assetId를 DELETE하면 그 보드 이미지가 깨진다.** 교체는 새 업로드 → assetId 스왑 순서로.

### 모니터 배정 (어느 기기에 어떤 보드를 띄울지)
```
GET    /api/devices/:deviceId/monitors
PATCH  /api/monitors/:id/screen          # {"screenId": 106}  단일 화면
PATCH  /api/monitors/:id/rotation        # {"screenIds":[104,105,106],"intervalMs":18000,"fadeMs":900}
```
- rotation: `screenIds` 순서대로 순환. `intervalMs` 2000~3600000, `fadeMs` 200~10000.
- `screenIds: []` = 로테이션 해제. 로테이션 시간만 바꾸려면 이 PATCH만 다시 호출.

### 기기 목록 / 원격 명령
```
GET    /api/devices                      # 기기+모니터+설치버전+online 상태
POST   /api/devices/:id/commands         # {"type":"updateApp"}  앱 OTA 트리거
```

---

## 3. ScreenItem 스키마 (layout.items[] 의 한 요소)

공통 필수: `id`(문자열, 유니크), `kind`(`image`|`video`|`text`), `x`,`y`,`width`,`height`(1920×1080 기준).

### 텍스트 아이템 (`kind:"text"`) — `textVariant`로 형태 결정
| textVariant | 용도 | 핵심 필드 |
|---|---|---|
| `plain`(기본) | 제목/자유 텍스트 | `text`, `fontSize`, `fontUnit`, `color`, `fontWeight`, `textAlign` |
| `menuLine` | **메뉴 한 줄** | `text`(한글명), `textEn`(영문), `textSecondary`(가격), `priceExtra`(보조가), `priceColor`, `badges[]` |
| `groupHeader` | 카테고리 헤더 | `text`(한글), `textEn`, `textSecondary`(우측 라벨) |
| `note` | 라운드 안내박스 | `text`, `background`(틴트), `color` |

기타 필드: `fontUnit`(`px`=1920×1080 디자인 px[기본], `vh`=화면높이%), `background`, `opacity`,
`lineHeight`, `radius`(모서리 둥글기 px — 그룹 카드/패널), `zIndex`.

### 이미지 아이템 (`kind:"image"`)
`assetId`(업로드한 에셋 id) + `x/y/width/height`. 렌더러가 `object-fit: cover`로 채운다.

### 색 표기
`#rrggbb` 또는 `rgba()/rgb()`. **rgba는 렌더러 파서(`parseColorOrNull`)를 반드시 거친다** —
과거 hex만 지원해 카드 틴트가 투명하게 나온 버그가 있었다. 박스 디스플레이는 저알파를
워시아웃하므로 카드 틴트는 0.13~0.17, 안내박스 0.30 정도로 진하게.

### 뱃지 (`badges[]`)
`{"text":"BEST","variant":"best"}`. variant: `best`(파란원)/`rec`(초록원)/`info`(회색 이탤릭)/`warn`(초록박스).
`best`/`rec`은 라벨 앞, `info`/`warn`은 뒤에 렌더.

---

## 4. 실전 레시피 (검증된 예시)

base 는 `API=http://100.96.152.109:7800/api` 로 가정.

### A. 가격 하나 바꾸기 (가장 흔한 작업)
"BAKERY 3(id=106)의 소금빵 가격을 2,800 → 3,000으로."
```bash
# 1) 현재 layout 가져오기
curl -s "$API/screens/106" > /tmp/s.json
# 2) menuLine 중 text=="소금빵" 항목의 textSecondary 를 "3.0" 으로 수정
python3 - <<'PY'
import json
s=json.load(open("/tmp/s.json"))
for it in s["layout"]["items"]:
    if it.get("textVariant")=="menuLine" and it.get("text")=="소금빵":
        it["textSecondary"]="3.0"      # 표기 규칙: 천단위 소수(3,000 -> "3.0")
json.dump({"layout":s["layout"]}, open("/tmp/patch.json","w"), ensure_ascii=False)
PY
# 3) 통째로 PATCH (layout 은 전체 교체)
curl -s -X PATCH "$API/screens/106" -H "Content-Type: application/json" -d @/tmp/patch.json
# → 수 초 내 해당 박스 화면에 반영
```
> 가격 표기는 천단위 소수 규칙: 4,000→`"4.0"`, +1,000(보조가)→`priceExtra:"+1.0"`.

### B. 메뉴 항목 추가/삭제
`layout.items` 배열에서 해당 `menuLine` 오브젝트를 추가/제거한다. 추가 시 **기존 항목을 복제**해
`id`(유니크 문자열), `text/textEn/textSecondary`, `y`(세로 위치)만 바꾸는 게 안전하다.
같은 컬럼의 항목들과 `x/width/height/fontSize`를 맞춰야 정렬이 유지된다.

### C. 이미지 교체
```bash
NEW=$(curl -s -X POST "$API/assets/upload" -F "file=@/path/new-bread.jpg" -F "storeId=1")
NID=$(echo "$NEW" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
# 화면 layout에서 kind=="image" 항목의 assetId 를 $NID 로 바꿔 PATCH (레시피 A와 동일 패턴)
```

### D. 새 보드 만들어 배정 (레이아웃 생성기 사용)
맨손 좌표 대신 `scripts/brotwerk/seed_boards.py`의 `bakery_layout()`/`beverage_layout()`를 쓴다.
데이터는 `menu.json` 수정 → 시드 실행이 **가장 안전한 워크플로우**. 절차는
[signage-guide.md §4.3](./signage-guide.md) 참고.

### E. 어느 기기에 무엇이 떠 있나 / 배정 바꾸기
```bash
curl -s "$API/devices" | python3 -c "
import sys,json
for d in json.load(sys.stdin):
  for m in d.get('monitors') or []:
    print(d['hardwareId'], 'monitorId=',m['id'], 'rotation=',m.get('rotationScreenIds'))"
# 특정 박스(monitorId=9)에 보드 3장 로테이션 배정:
curl -s -X PATCH "$API/monitors/9/rotation" -H "Content-Type: application/json" \
  -d '{"screenIds":[104,105,106],"intervalMs":18000,"fadeMs":900}'
```

---

## 5. 안전 수칙 (에이전트가 지킬 것)

1. **PATCH 전 항상 GET.** layout은 통째 교체다. 최신 상태를 받아 수정 후 되돌려보낼 것.
2. **변경 전 백업 인지.** 서버는 매일 04:00 구글드라이브 자동 백업(`ddadan-backup.sh`). 큰 변경 전
   해당 screen을 `GET`해서 원본 JSON을 남겨두면 즉시 롤백 가능.
3. **DELETE는 신중히.** 로테이션에 물린 screen을 지우면 그 박스가 해당 슬라이드에서 깨진다.
   먼저 `GET /devices`로 어느 monitor의 `rotationScreenIds`에 있는지 확인.
4. **assetId 스왑은 새 업로드 → 교체 → (원하면) 옛 에셋 삭제** 순서.
5. **좌표는 1920×1080**. 다른 값으로 잡으면 비율이 틀어진다.
6. **반영 확인**: 변경 후 `GET /api/player/{hardwareId}/screen?slot=0`으로 박스가 받을 실제 페이로드를
   조회해 의도대로 나오는지 검증. 필요하면 스크린샷 명령(`POST /devices/:id/commands` 계열)도 활용.

---

## 6. 빠른 참조 — 현재 배치 (2026-07 기준, 변동 가능)

| 기기 | monitorId | 용도 |
|---|---|---|
| `U4XCSTB...025` | 8 | 매장 박스 |
| `U4XCSTB...034` | 9 | 매장 박스 |
| `U4XCSTB...031` | 10 | 매장 박스 |

실제 값은 항상 `GET /api/devices`로 확인할 것(이 표는 스냅샷).
