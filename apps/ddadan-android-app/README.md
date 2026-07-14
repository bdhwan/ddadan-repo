# DDADAN Android Player

Android TV 박스용 디지털 사이니지 플레이어. [ddadan-client-app](../ddadan-client-app)과 동일한 API를 사용합니다.

## 요구사항

- Android CLI (`android` 명령)
- JDK 17+
- Android SDK (프로젝트 생성 시 자동 설치)

## 빌드

```bash
# 모노레포 루트에서
npm run build:android

# 또는 직접
cd apps/ddadan-android-app && ./gradlew assembleDebug
```

## 실행

```bash
npm run run:android
```

에뮬레이터 기본 API 주소: `http://10.0.2.2:7800/api` (호스트의 localhost:7800)

## 설정

| 항목 | 기본값 | 저장 |
|------|--------|------|
| 등록코드 (deviceId) | `dev-local` | DataStore |
| 슬롯 (slot) | `0` | DataStore |
| API 서버 | BuildConfig | DataStore 오버라이드 가능 |

앱 내 **설정 (Menu)** 배지 또는 리모컨 Menu 키로 API 서버/슬롯을 변경할 수 있습니다.

### Intent extra (MDM 배포)

```bash
adb shell am start -n com.ddadan.player/.MainActivity \
  --es deviceId display-1 \
  --ei slot 0 \
  --es apiBase "http://192.168.0.10:7800/api"
```

### Release API 기본값

`gradle.properties`의 `DDADAN_API_BASE`를 설정합니다.

## API

```
GET {apiBase}/player/{hardwareId}/screen?slot={slot}
```

## 테스트

```bash
cd apps/ddadan-android-app && ./gradlew test
```
