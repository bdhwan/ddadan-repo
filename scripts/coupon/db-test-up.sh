#!/usr/bin/env bash
set -euo pipefail

# 같은 Postgres 컨테이너 안에 테스트 전용 DB(coupon_test)를 만들고
# 마이그레이션을 적용한다. 컨테이너를 새로 띄우지 않으며, 개발용 DB(coupon)는 건드리지 않는다.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
COMPOSE_FILE="${REPO_ROOT}/apps/coupon-api-server/docker-compose.dev.yml"
MIGRATIONS_DIR="${REPO_ROOT}/apps/coupon-api-server/migrations"
TEST_DB="coupon_test"
DEV_DB="coupon"
PG_USER="coupon"
PG_PASSWORD="coupon_dev_password"
PG_HOST="localhost"
PG_PORT="55432"
TEST_DATABASE_URL="postgres://${PG_USER}:${PG_PASSWORD}@${PG_HOST}:${PG_PORT}/${TEST_DB}"

if [ ! -f "${COMPOSE_FILE}" ]; then
  echo "오류: compose 파일이 없습니다: ${COMPOSE_FILE}" >&2
  exit 1
fi

if [ ! -d "${MIGRATIONS_DIR}" ]; then
  echo "오류: 마이그레이션 디렉터리가 없습니다: ${MIGRATIONS_DIR}" >&2
  exit 1
fi

if ! command -v sqlx >/dev/null 2>&1; then
  echo "오류: sqlx-cli 가 PATH 에 없습니다. cargo install sqlx-cli --no-default-features --features rustls,postgres" >&2
  exit 1
fi

if ! docker compose -f "${COMPOSE_FILE}" ps --status running --services 2>/dev/null | grep -qx postgres; then
  echo "오류: postgres 컨테이너가 실행 중이 아닙니다. 먼저 ./scripts/coupon/db-up.sh 를 실행하세요." >&2
  exit 1
fi

if [ "${TEST_DB}" = "${DEV_DB}" ]; then
  echo "오류: 테스트 DB 이름이 개발용 DB 와 같습니다. 중단합니다." >&2
  exit 1
fi

compose_psql() {
  docker compose -f "${COMPOSE_FILE}" exec -T postgres \
    psql -U "${PG_USER}" -d postgres -v ON_ERROR_STOP=1 "$@"
}

exists="$(compose_psql -Atc "SELECT 1 FROM pg_database WHERE datname = '${TEST_DB}'" || true)"
if [ "${exists}" = "1" ]; then
  echo "테스트 DB '${TEST_DB}' 가 이미 있습니다. 마이그레이션만 적용합니다."
  echo "(개발용 DB '${DEV_DB}' 는 변경하지 않습니다.)"
else
  echo "테스트 DB '${TEST_DB}' 를 생성합니다. (개발용 DB '${DEV_DB}' 는 변경하지 않습니다.)"
  compose_psql -c "CREATE DATABASE ${TEST_DB} OWNER ${PG_USER};"
fi

echo "마이그레이션 적용: ${TEST_DATABASE_URL}"
# DB 역할명과 도메인 스키마가 둘 다 coupon 이라 기본 search_path("$user", public) 에서는
# 마이그레이션 추적 테이블이 coupon 스키마로 새어 재적용이 반복된다. migrate 만 public 고정.
(
  cd "${REPO_ROOT}/apps/coupon-api-server"
  PGOPTIONS='-c search_path=public' DATABASE_URL="${TEST_DATABASE_URL}" sqlx migrate run
)

echo "---"
echo "테스트 DB 준비 완료: ${TEST_DATABASE_URL}"
echo "개발용 DB(${DEV_DB})는 그대로입니다."
