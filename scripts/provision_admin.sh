#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

: "${DATABASE_URL:?DATABASE_URL is required}"
: "${HOSPITAL_ID:?HOSPITAL_ID is required}"
: "${USERNAME:?USERNAME is required}"
: "${PASSWORD:?PASSWORD is required}"

cargo run --quiet --manifest-path "$PWD/app/Cargo.toml" --bin provision_admin
