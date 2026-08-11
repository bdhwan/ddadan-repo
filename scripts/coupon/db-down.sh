#!/usr/bin/env bash
set -euo pipefail

# Postgres/Redis 컨테이너 중지 (볼륨은 지우지 않음)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
COMPOSE_FILE="${REPO_ROOT}/apps/coupon-api-server/docker-compose.dev.yml"

if [ ! -f "${COMPOSE_FILE}" ]; then
  echo "compose 파일이 아직 없습니다: ${COMPOSE_FILE}"
  exit 1
fi

docker compose -f "${COMPOSE_FILE}" down
