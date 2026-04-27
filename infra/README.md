# DDADAN — Portainer Stack 배포

`infra/portainer-stack.yml`은 Portainer Stacks UI에서 바로 띄울 수 있는
운영 배포 정의이다. 컨테이너 5개 (`mysql`, `redis`, `api`, `admin`,
`player`)가 단일 docker bridge 네트워크에서 동작하며 어드민/플레이어
nginx가 `/api/`와 `/static/` 요청을 api 컨테이너로 프록시한다.

## 1. 배포 모드

### A. Repository 모드 (권장)
이 저장소를 가리키게 하면 Portainer가 자동으로 빌드한다.

| 필드 | 값 |
|---|---|
| **Build method** | Repository |
| **Repository URL** | `https://github.com/bdhwan/ddadan-repo` (또는 사용 중인 origin) |
| **Repository reference** | `refs/heads/main` (또는 배포할 브랜치) |
| **Compose path** | `infra/portainer-stack.yml` |
| **Authentication** | private 저장소면 PAT 토큰 입력 |

빌드 컨텍스트는 `..`(저장소 루트)로 잡혀 있으므로 모노레포 의존성이 정상 해석된다.

### B. Web editor 모드
Web editor에서는 `build: ...`이 동작하지 않는다. 이 경우:

1. CI에서 세 이미지를 미리 빌드하여 레지스트리에 푸시한다.
   ```bash
   docker build -f apps/ddadan-api-server/Dockerfile  -t <reg>/ddadan-api:<tag>    .
   docker build -f apps/ddadan-admin-app/Dockerfile   -t <reg>/ddadan-admin:<tag>  .
   docker build -f apps/ddadan-client-app/Dockerfile  -t <reg>/ddadan-player:<tag> .
   ```
2. `portainer-stack.yml`의 각 `build: ...` 블록을 아래처럼 교체한다.
   ```yaml
   api:
     image: <reg>/ddadan-api:<tag>
   admin:
     image: <reg>/ddadan-admin:<tag>
   player:
     image: <reg>/ddadan-player:<tag>
   ```

## 2. 환경변수 (Portainer Stack form)

**필수**

| 변수 | 설명 |
|---|---|
| `FIREBASE_SERVICE_ACCOUNT_JSON` | Firebase Admin 서비스 계정 JSON **전체 본문**을 그대로 붙여넣기. 컨테이너 entrypoint가 `/app/firebase/service-account.json`으로 기록해 firebase-admin이 사용한다. |
| `MYSQL_ROOT_PASSWORD` | MySQL root 비밀번호. 기본값(`change_me_root`)은 운영용이 아님. |
| `MYSQL_PASSWORD` | `ddadan` 사용자 비밀번호. api/mysql 양쪽에서 동일하게 사용. |

**선택**

| 변수 | 기본값 | 설명 |
|---|---|---|
| `API_PORT` | `3000` | api 컨테이너 호스트 노출 포트 |
| `ADMIN_PORT` | `4200` | admin nginx 호스트 노출 포트 |
| `PLAYER_PORT` | `4300` | player nginx 호스트 노출 포트 |
| `CORS_ORIGINS` | `http://localhost:4200,http://localhost:4300` | api가 허용할 어드민/플레이어 origin. 운영 도메인을 콤마로 나열. |
| `DB_SYNCHRONIZE` | `true` | TypeORM `synchronize`. 스키마 안정화 후 `false`로 바꾸고 마이그레이션 적용 권장. |
| `DB_LOGGING` | `false` | SQL 로그 표준출력 |
| `HEARTBEAT_OFFLINE_AFTER_SECONDS` | `60` | 디바이스 하트비트가 이 초만큼 끊기면 offline으로 마킹 |
| `MYSQL_USER`, `MYSQL_DATABASE` | `ddadan` | 컨테이너 부트 시 생성될 사용자/DB |

## 3. 주의 사항

- **Firebase Auth 도메인 등록**: Firebase Console → Authentication → Settings →
  Authorized domains에 운영 어드민 도메인을 추가해야 구글 로그인 팝업이 동작한다.
- **CORS_ORIGINS**: 어드민/플레이어가 다른 도메인이라면 모두 등록해야 한다.
- **에셋 영속성**: `api-assets` 볼륨이 업로드 파일을 보관한다. 백업 대상.
- **Firebase 서비스 계정 영속성**: `api-firebase` 볼륨에 entrypoint가 JSON을
  쓰며, 재시작 시 env 값이 비어 있어도 기존 파일이 남아 있어 작동한다.
- **포트 충돌**: 기본값(3000/4200/4300/3306)이 호스트에서 겹치면
  `*_PORT` 변수로 변경.
- **HTTPS / 도메인**: 본 스택은 평문 HTTP만 노출한다. 프론트(Caddy/Traefik
  /nginx-proxy 등) 리버스 프록시를 별도로 두고 인증서를 발급하거나,
  Portainer가 같은 노드에 띄운 별도 프록시 스택과 같은 외부 네트워크를
  공유하도록 구성하라.
- **Pi 디바이스**: 본 스택에 포함되지 않는다. 각 라즈베리파이는
  `services/ddadan-service-pi`를 직접 실행하면서
  `DDADAN_API_BASE`, `DDADAN_ADMIN_BASE`, `DDADAN_PLAYER_BASE`를
  공개 도메인으로 설정한다.

## 4. 헬스 체크

배포 후 다음을 확인한다.

```bash
# api 컨테이너 로그에 "Nest application successfully started"가 보이는지
# (Portainer → Containers → api → Logs)

# 외부에서 시드된 약관이 내려오는지
curl http://<host>:${ADMIN_PORT}/api/policies/current

# 어드민 SPA가 뜨는지
curl -I http://<host>:${ADMIN_PORT}/

# 플레이어 fallback 화면이 뜨는지 (deviceId가 미등록일 때)
curl http://<host>:${PLAYER_PORT}/api/player/test-device/screen
```

## 5. 업데이트 절차

Repository 모드에서는 Portainer Stack 화면의 **"Pull and redeploy"** 버튼이
브랜치 최신을 받아 다시 빌드/기동한다. Web editor 모드에서는 새 이미지
태그로 yml을 수정하고 **Update the stack**.
