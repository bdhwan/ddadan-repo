#!/bin/sh
set -e

# If the operator pasted the Firebase service-account JSON into an env var
# (FIREBASE_SERVICE_ACCOUNT_JSON), drop it on disk where firebase-admin expects it.
if [ -n "${FIREBASE_SERVICE_ACCOUNT_JSON:-}" ]; then
  mkdir -p /app/firebase
  printf '%s' "$FIREBASE_SERVICE_ACCOUNT_JSON" > /app/firebase/service-account.json
  chmod 600 /app/firebase/service-account.json
  echo "[entrypoint] wrote /app/firebase/service-account.json from env"
fi

exec "$@"
