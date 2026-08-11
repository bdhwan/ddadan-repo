#!/usr/bin/env bash
set -euo pipefail

# 테스트 전용 DB(coupon_test)만 drop 후 재생성하고 마이그레이션을 다시 적용한다.
# 개발용 DB(coupon)는 절대 건드리지 않는다.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
COMPOSE_FILE="${REPO_ROOT}/apps/coupon-api-server/docker-compose.dev.yml"
TEST_DB="coupon_test"
DEV_DB="coupon"
PG_USER="coupon"

if [ ! -f "${COMPOSE_FILE}" ]; then
  echo "오류: compose 파일이 없습니다: ${COMPOSE_FILE}" >&2
  exit 1
fi

if ! docker compose -f "${COMPOSE_FILE}" ps --status running --services 2>/dev/null | grep -qx postgres; then
  echo "오류: postgres 컨테이너가 실행 중이 아닙니다. 먼저 ./scripts/coupon/db-up.sh 를 실행하세요." >&2
  exit 1
fi

# 안전장치: 대상이 반드시 테스트 DB 이름이어야 한다.
if [ "${TEST_DB}" = "${DEV_DB}" ] || [ "${TEST_DB}" != "coupon_test" ]; then
  echo "오류: 예상치 못한 테스트 DB 이름 '${TEST_DB}'. 개발용 DB 보호를 위해 중단합니다." >&2
  exit 1
fi

compose_psql() {
  docker compose -f "${COMPOSE_FILE}" exec -T postgres \
    psql -U "${PG_USER}" -d postgres -v ON_ERROR_STOP=1 "$@"
}

echo "=== 테스트 DB 리셋 ==="
echo "대상: ${TEST_DB} 만 drop 합니다. 재생성·마이그레이션은 db-test-up.sh 가 담당합니다."
echo "개발용 DB(${DEV_DB})는 건드리지 않습니다."
echo ""

# 열린 연결을 끊은 뒤 drop (테스트 DB만). 없으면 무시.
compose_psql -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '${TEST_DB}' AND pid <> pg_backend_pid();" >/dev/null || true
compose_psql -c "DROP DATABASE IF EXISTS ${TEST_DB};"

"${SCRIPT_DIR}/db-test-up.sh"

echo "---"
echo "테스트 DB 리셋 완료. 개발용 DB(${DEV_DB})는 변경되지 않았습니다."
