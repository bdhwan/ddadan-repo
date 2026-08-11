#!/usr/bin/env bash
set -euo pipefail

# 볼륨까지 삭제 후 재기동 (데이터 전부 삭제)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
COMPOSE_FILE="${REPO_ROOT}/apps/coupon-api-server/docker-compose.dev.yml"

if [ ! -f "${COMPOSE_FILE}" ]; then
  echo "compose 파일이 아직 없습니다: ${COMPOSE_FILE}"
  exit 1
fi

echo "데이터가 모두 삭제됩니다. 계속하려면 yes 입력:"
read -r CONFIRM

if [ "${CONFIRM}" != "yes" ]; then
  echo "취소되었습니다."
  exit 1
fi

docker compose -f "${COMPOSE_FILE}" down -v
docker compose -f "${COMPOSE_FILE}" up -d

echo "---"
docker compose -f "${COMPOSE_FILE}" ps
