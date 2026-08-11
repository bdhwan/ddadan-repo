#!/usr/bin/env bash
set -euo pipefail

# 테스트 전용 DB(coupon_test)를 가리키는 COUPON_TEST_DATABASE_URL 을 설정한 뒤
# cargo test --workspace 를 실행한다.
# 이 변수가 없으면 통합 테스트가 조용히 skip 되므로 반드시 설정한다.
# 개발용 DB(coupon)는 사용하지 않는다.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TEST_DB="coupon_test"
DEV_DB="coupon"
PG_USER="coupon"
PG_PASSWORD="coupon_dev_password"
PG_HOST="localhost"
PG_PORT="55432"
export COUPON_TEST_DATABASE_URL="postgres://${PG_USER}:${PG_PASSWORD}@${PG_HOST}:${PG_PORT}/${TEST_DB}"

if [ "${TEST_DB}" = "${DEV_DB}" ]; then
  echo "오류: 테스트 DB 가 개발용 DB 와 같습니다. 중단합니다." >&2
  exit 1
fi

echo "테스트 DB 준비 확인 중..."
if ! "${SCRIPT_DIR}/db-test-up.sh"; then
  echo "오류: 테스트 DB 준비에 실패했습니다. 위 메시지를 확인하세요." >&2
  exit 1
fi

echo ""
echo "=== cargo test --workspace ==="
echo "COUPON_TEST_DATABASE_URL=${COUPON_TEST_DATABASE_URL}"
echo "(개발용 DB ${DEV_DB} 는 사용하지 않습니다.)"
echo ""

cd "${REPO_ROOT}"
cargo test --workspace "$@"
