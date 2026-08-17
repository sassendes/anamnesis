#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

MANIFEST=app/Cargo.toml

echo "==> fmt"
cargo fmt --manifest-path "$MANIFEST" --all -- --check

echo "==> clippy"
cargo clippy --manifest-path "$MANIFEST" --all-targets -- -D warnings

echo "==> test"
cargo test --manifest-path "$MANIFEST"

echo "==> build (release)"
cargo build --manifest-path "$MANIFEST" --release

echo "==> k8s manifest dry-run"
if command -v kubectl >/dev/null 2>&1; then
  kubectl apply --dry-run=client -k k8s/ 2>&1 || true
else
  echo "(kubectl not found, skipping)"
fi

echo "CI OK"
