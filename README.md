# ddadan-repo

소상공인용 디지털 사이니지 SaaS + 하드웨어 서비스. NestJS API + Angular 어드민/플레이어 + Raspberry Pi 로컬 서비스로 구성된 모노레포.

## 구성

- `apps/ddadan-api-server`
  - NestJS API 서버
  - MySQL(TypeORM) + Redis + BullMQ + Firebase Admin SDK
  - 매장/디바이스/모니터/에셋/화면/하트비트/약관/회원탈퇴 도메인
  - 정적 에셋 서빙 (`/static/assets/*`)
- `apps/ddadan-admin-app`
  - 매장/디바이스 관리, 모니터 드래그드롭 배치, PPT식 화면 편집기
  - Firebase Web SDK 이메일/구글 로그인
  - Angular 21 (port 4200)
- `apps/ddadan-client-app`
  - 라즈베리파이 풀스크린 플레이어
  - `?deviceId=<hardwareId>` 쿼리로 본인 화면 layout JSON을 폴링
  - Angular 21 (port 4300 권장)
- `services/ddadan-service-pi`
  - 하드웨어 ID 자동 인식 (machine-id / eth0 mac)
  - 미등록 시 어드민 `/register?deviceId=...` 자동 진입
  - 등록 시 플레이어 URL을 Chromium kiosk 모드로 실행
  - 주기 하트비트 + 모니터 해상도 자동 보고

## 빠른 시작

```bash
# 1) 인프라(MySQL, Redis) 기동
npm run infra:up

# 2) 의존성 설치
npm install

# 3) Firebase 서비스 계정 JSON을 apps/ddadan-api-server/firebase/service-account.json 에 배치
#    (예시는 service-account.example.json 참고)
cp apps/ddadan-api-server/.env.example apps/ddadan-api-server/.env
cp services/ddadan-service-pi/.env.example services/ddadan-service-pi/.env

# 4) 각 워크스페이스 개발 서버
npm run dev:api      # NestJS, http://localhost:7800/api
npm run dev:admin    # Angular 어드민, http://localhost:4200
npm run dev:client   # Angular 플레이어 (다른 포트로 실행 권장)
npm run dev:pi       # Pi 서비스 (DDADAN_LAUNCH_KIOSK=0 으로 dry-run 가능)
```

> 프론트의 Firebase Web SDK 키는 `apps/ddadan-admin-app/src/environment.ts`에 입력해야 한다.

## 주요 API

| Method | Path | 설명 |
|---|---|---|
| `POST /api/devices/check` | public | Pi가 부팅 시 등록 여부 확인 |
| `POST /api/devices/heartbeat` | public | Pi 주기 하트비트 + 해상도 보고 |
| `POST /api/devices` | auth | 어드민에서 매장에 디바이스 등록 |
| `GET /api/stores/:storeId/devices` | auth | 매장별 디바이스 목록 |
| `PATCH /api/monitors/:id/position` | auth | 모니터 드래그드롭 위치 갱신 |
| `PATCH /api/monitors/:id/screen` | auth | 모니터에 화면 할당 |
| `POST /api/assets/upload` | auth | 이미지/영상 업로드 (multer disk) |
| `POST /api/assets/text` | auth | 텍스트 에셋 |
| `GET/POST/PATCH /api/screens` | auth | 화면 CRUD (PPT식 layout JSON) |
| `POST /api/screen-components` | auth | 재사용 컴포넌트 저장 |
| `GET /api/player/:hardwareId/screen?slot=0` | public | 플레이어가 풀스크린에 그릴 layout |
| `GET /api/policies/current` | public | 약관/개인정보 최신 버전 |
| `POST /api/policies/accept` | auth | 가입 시 약관 동의 기록 |
| `DELETE /api/account` | auth | 회원 탈퇴 (전 데이터 soft delete + Firebase auth user 삭제) |

## 데이터 모델 요약

`users → stores → devices → monitors`. 각 모니터에 `current_screen_id`로 `screens`를 연결.
`screens.layout`은 PPT 슬라이드처럼 절대 위치 + 크기 + 종류(image/video/text)를 가진 아이템 리스트.
`screen_components`는 사용자가 만든 재사용 가능한 그룹/단일 컴포넌트.

## 회원 탈퇴 정책

- 탈퇴 시 `users / stores / devices / monitors / assets / screens / screen_components` 모두 soft delete
- Firebase Auth 사용자도 함께 삭제
- 동일 이메일로 즉시 재가입 가능 (새 firebase_uid 발급으로 자연스러운 새 user row 생성)
