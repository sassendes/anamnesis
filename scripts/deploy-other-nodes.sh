#!/usr/bin/env bash
# Run on each node that is NOT cp1 (e.g. .66 and .67). Pulls both image tars
# from cp1's http server and imports them into this node's k3s containerd.
#
# Usage:  ./scripts/deploy-other-nodes.sh <CP1_IP>
#   e.g.  ./scripts/deploy-other-nodes.sh 192.168.100.65
set -euo pipefail

CP1_IP="${1:?need cp1 ip, e.g. ./scripts/deploy-other-nodes.sh 192.168.100.65}"
BASE="http://${CP1_IP}:8000"

cd /tmp
echo ">> pulling images from ${BASE}"
wget -q "${BASE}/anamnesis.tar" -O anamnesis.tar
wget -q "${BASE}/anamnesis-web.tar" -O anamnesis-web.tar

echo ">> importing into local k3s containerd"
k3s ctr images import anamnesis.tar
k3s ctr images import anamnesis-web.tar

echo ">> cleaning up"
rm -f anamnesis.tar anamnesis-web.tar

echo ">> done on $(hostname). images imported."
