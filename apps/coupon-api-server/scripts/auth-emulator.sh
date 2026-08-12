#!/usr/bin/env bash
#
# Firebase Auth emulator (기획서 §20.1 `local` 환경, §19.3 계약 테스트).
#
# 실제 Firebase 프로젝트 없이 이메일/비밀번호 가입 → ID Token 발급 → 서버 검증까지
# 전 구간을 돌리기 위한 것이다. Auth emulator 는 Node 만 있으면 되고 Java 는 필요 없다.
#
#   ./apps/coupon-api-server/scripts/auth-emulator.sh up      # 백그라운드 기동
#   ./apps/coupon-api-server/scripts/auth-emulator.sh status  # 살아있는지 확인
#   ./apps/coupon-api-server/scripts/auth-emulator.sh logs
#   ./apps/coupon-api-server/scripts/auth-emulator.sh down
#
# **0.0.0.0 에 바인딩한다.** 실기기(휴대폰)가 LAN 으로 붙어야 하므로 localhost 전용으로
# 띄우면 §19.5 실기기 검증이 불가능해진다. 바인딩 주소는 firebase/firebase.json 에 있다.
#
# 서버 쪽에서는 COUPON_FIREBASE_AUTH_EMULATOR_HOST 를 아래 EMULATOR_HOST 와 같은 값으로
# 두면 emulator 가 발급한 ID Token 을 받아들인다. 이 설정은 production 에서 부팅을
# 거부하므로(§16.3, COUPON_AUTH_DEV_BYPASS 와 같은 취급) 실수로 남아 있어도 배포는 막힌다.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIREBASE_DIR="${SCRIPT_DIR}/../firebase"
RUN_DIR="${TMPDIR:-/tmp}/coupon-auth-emulator"
PID_FILE="${RUN_DIR}/emulator.pid"
LOG_FILE="${RUN_DIR}/emulator.log"

# LAN 주소. 실기기와 클라이언트 담당이 같은 값을 쓴다(코디네이터 확정).
EMULATOR_LAN_HOST="${COUPON_EMULATOR_LAN_HOST:-192.168.150.185}"
AUTH_PORT="${COUPON_EMULATOR_AUTH_PORT:-9099}"
# 코디네이터 계약은 UI 4400 이었으나 이 개발 머신에서는 4400 을 무관한 http-server 가 이미
# 쓰고 있어 4410 으로 옮겼다. 계약의 본체인 Auth 포트 9099 는 그대로다.
UI_PORT="${COUPON_EMULATOR_UI_PORT:-4410}"
PROJECT_ID="${COUPON_FIREBASE_PROJECT_ID:-ddadan-dev}"

emulator_host() { echo "${EMULATOR_LAN_HOST}:${AUTH_PORT}"; }

is_up() {
  curl -fsS --max-time 2 "http://127.0.0.1:${AUTH_PORT}/" >/dev/null 2>&1
}

require_cli() {
  if ! command -v firebase >/dev/null 2>&1; then
    cat >&2 <<'EOF'
오류: firebase CLI 가 없습니다.

  npm install -g firebase-tools

Auth emulator 는 Node 만 필요합니다(Firestore 와 달리 Java 는 필요 없습니다).
EOF
    exit 1
  fi
}

case "${1:-up}" in
  up)
    require_cli
    if is_up; then
      echo "이미 떠 있습니다: http://$(emulator_host)"
      exit 0
    fi
    mkdir -p "${RUN_DIR}"
    echo "Auth emulator 기동 중 (project=${PROJECT_ID}, 0.0.0.0:${AUTH_PORT})..."
    (
      cd "${FIREBASE_DIR}"
      nohup firebase emulators:start --only auth --project "${PROJECT_ID}" \
        >"${LOG_FILE}" 2>&1 &
      echo $! >"${PID_FILE}"
    )
    for _ in $(seq 1 60); do
      if is_up; then
        echo "준비됨."
        echo "  Auth : http://$(emulator_host)   (UI: http://${EMULATOR_LAN_HOST}:${UI_PORT})"
        echo "  서버 : COUPON_FIREBASE_AUTH_EMULATOR_HOST=$(emulator_host)"
        echo "         COUPON_FIREBASE_PROJECT_ID=${PROJECT_ID}"
        echo "  로그 : ${LOG_FILE}"
        exit 0
      fi
      sleep 1
    done
    echo "오류: emulator 가 60초 안에 응답하지 않았습니다. 로그: ${LOG_FILE}" >&2
    tail -30 "${LOG_FILE}" >&2 || true
    exit 1
    ;;
  down)
    if [ -f "${PID_FILE}" ]; then
      pid="$(cat "${PID_FILE}")"
      # firebase CLI 는 자식 프로세스(java/node)를 두므로 프로세스 그룹째 내린다.
      kill -TERM -- "-${pid}" 2>/dev/null || kill -TERM "${pid}" 2>/dev/null || true
      rm -f "${PID_FILE}"
    fi
    pkill -f "firebase.*emulators:start.*--only auth" 2>/dev/null || true
    echo "내렸습니다."
    ;;
  status)
    if is_up; then
      echo "up   http://$(emulator_host)"
    else
      echo "down"
      exit 1
    fi
    ;;
  logs)
    tail -f "${LOG_FILE}"
    ;;
  host)
    emulator_host
    ;;
  *)
    echo "사용법: $0 {up|down|status|logs|host}" >&2
    exit 2
    ;;
esac
