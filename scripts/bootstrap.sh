#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

./scripts/setup-cluster.sh operators
kubectl apply -k k8s/

echo ">> waiting for postgres..."
until kubectl get cluster anamnesis -o jsonpath='{.status.readyInstances}' 2>/dev/null | grep -q 3; do sleep 5; done

./scripts/seed-patients.sh 1000

if [ "${1:-}" = "admin" ]; then
  ./scripts/create-admin.sh "${2:?hospital_id}" "${3:?username}" "${4:?argon2 hash}"
fi

echo ">> done."
