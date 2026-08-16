#!/usr/bin/env bash
# End-to-end smoke test against a throwaway postgres: migrate + seed as the
# superuser, then run the API as the least-privileged app role (so RLS is live)
# and drive login -> create -> read over HTTP.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONTAINER=anamnesis-smoke-db
HOSPITAL_ID=11111111-1111-1111-1111-111111111111
PASSWORD='Doctor123!'
PORT=18080

ADMIN_URL="postgres://postgres:smoke@localhost:54333/anamnesis?sslmode=disable"
APP_URL="postgres://anamnesis_app:root@localhost:54333/anamnesis?sslmode=disable"

cleanup() {
  [ -n "${SERVER_PID:-}" ] && kill "$SERVER_PID" 2>/dev/null || true
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "==> starting postgres"
docker run -d --name "$CONTAINER" \
  -e POSTGRES_PASSWORD=smoke -e POSTGRES_DB=anamnesis \
  -p 54333:5432 postgres:17-alpine >/dev/null

for _ in $(seq 1 30); do
  docker exec "$CONTAINER" pg_isready -U postgres -q && break
  sleep 1
done

echo "==> migrations (as superuser; creates the anamnesis_app role + grants)"
ANAMNESIS_RUN_MIGRATIONS=1 \
  ANAMNESIS_DATABASE_URL="$ADMIN_URL" \
  ANAMNESIS_JWT_SECRET=smoke_secret_at_least_32_bytes_long \
  cargo run --quiet --manifest-path "$ROOT/app/Cargo.toml"

echo "==> provision admin (as superuser)"
HOSPITAL_ID=$HOSPITAL_ID USERNAME=sysadmin PASSWORD="$PASSWORD" \
  DATABASE_URL="$ADMIN_URL" \
  cargo run --quiet --manifest-path "$ROOT/app/Cargo.toml" --bin provision_admin

echo "==> server (as least-privileged app role)"
ANAMNESIS_DATABASE_URL="$APP_URL" \
  ANAMNESIS_JWT_SECRET=smoke_secret_at_least_32_bytes_long \
  ANAMNESIS_LISTEN_ADDR="127.0.0.1:$PORT" ANAMNESIS_METRICS_ADDR=127.0.0.1:19090 \
  cargo run --quiet --manifest-path "$ROOT/app/Cargo.toml" &
SERVER_PID=$!

for i in $(seq 1 30); do
  curl -fsS "http://127.0.0.1:$PORT/api/v1/healthz" >/dev/null && break
  [ "$i" = 30 ] && { echo "server did not become healthy"; exit 1; }
  sleep 1
done
echo "health ok"

echo "==> readyz"
curl -fsS "http://127.0.0.1:$PORT/api/v1/readyz"
echo

echo "==> login"
TOKEN=$(curl -fsS -X POST "http://127.0.0.1:$PORT/api/v1/auth/login" \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"sysadmin\",\"password\":\"$PASSWORD\",\"hospital_id\":\"$HOSPITAL_ID\"}" |
  python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])')
echo "token ok (len=${#TOKEN})"

echo "==> create patient"
PATIENT=$(curl -fsS -X POST "http://127.0.0.1:$PORT/api/v1/patients" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"full_name":"Ada Lovelace","sex":"F","birth_date":"1815-12-10","phone":"+33-1-5550-0100","address":"12 Somers Town"}')
echo "$PATIENT"
PATIENT_ID=$(echo "$PATIENT" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')

echo "==> search patients"
curl -fsS "http://127.0.0.1:$PORT/api/v1/patients?q=Ada&page=5" \
  -H "Authorization: Bearer $TOKEN" | python3 -m json.tool | head -8

echo
echo "SMOKE OK (patient $PATIENT_ID)"
