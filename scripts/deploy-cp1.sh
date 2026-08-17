#!/usr/bin/env bash
# Run on control1 (cp1). Builds both images, imports them locally, then serves
# the tars over http so the other nodes can pull them.
#
# Usage:  ./scripts/deploy-cp1.sh
#
# After this finishes it prints the exact command to run on the other nodes,
# then serves on :8000 until you Ctrl+C it. Roll the deployments after every
# node has imported (see the printed instructions).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo ">> building anamnesis:local (api)"
docker build -t anamnesis:local app/

echo ">> building anamnesis-web:local (web)"
docker build -t anamnesis-web:local web/

echo ">> saving tars"
docker save anamnesis:local -o anamnesis.tar
docker save anamnesis-web:local -o anamnesis-web.tar

echo ">> importing into local k3s containerd"
k3s ctr images import anamnesis.tar
k3s ctr images import anamnesis-web.tar

MYIP="$(hostname -I | awk '{print $1}')"
echo ""
echo ">> local import done. now run this on EACH other node:"
echo "   ./scripts/deploy-other-nodes.sh ${MYIP}"
echo ""
echo ">> once all nodes have imported, roll the deployments from cp1:"
echo "   k3s kubectl rollout restart deployment anamnesis-api anamnesis-web"
echo ""
echo ">> serving tars on :8000 (Ctrl+C to stop once nodes are done)..."
python3 -m http.server 8000
