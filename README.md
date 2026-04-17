# ddadan-repo

모노레포 구조의 디지털 사이니지 서비스입니다.

## 구성

- `apps/ddadan-api-server`
  - NestJS API 서버
  - 디바이스, 플레이리스트, 콘텐츠 배포 관리
- `apps/ddadan-client-app`
  - Angular 기반 사이니지 플레이어
  - 라즈베리파이에 연결된 모니터에서 전체화면으로 표시
- `services/ddadan-service-pi`
  - Raspberry Pi 디바이스 서비스
  - 부팅 시 앱 실행, 디바이스 등록, 상태 보고, kiosk 실행 담당

## 목표 구조

- 관리자는 웹 관리툴에서 콘텐츠와 디바이스를 관리
- API 서버는 디바이스별 재생 정책과 플레이리스트를 제공
- 라즈베리파이는 client app을 전체화면으로 띄워 메뉴판/광고판처럼 동작

## 워크스페이스 실행 예시

```bash
npm install
npm run dev:api
npm run dev:client
npm run dev:pi
```
