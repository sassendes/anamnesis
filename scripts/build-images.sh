#!/usr/bin/env bash
# Build the anamnesis api + web images with docker, then import them into
# k3s's containerd on every node (imagePullPolicy: Never needs the image
# present locally on each node the pod can land on).
#
# Run from the repo root:
#   ./scripts/build-images.sh                       # build + import on THIS node only
#   ./scripts/build-images.sh control2 control3     # also scp+import on those nodes over ssh
#
# Image tags are pinned to what k8s/app and k8s/web expect:
#   anamnesis:local   /   anamnesis-web:local
set -euo pipefail

API_IMAGE="anamnesis:local"
WEB_IMAGE="anamnesis-web:local"
API_TAR="anamnesis.tar"
WEB_TAR="anamnesis-web.tar"

# Resolve repo root from this script's location, so it works from anywhere.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo ">> building ${API_IMAGE} (context: app/)"
docker build -t "${API_IMAGE}" app/

echo ">> building ${WEB_IMAGE} (context: web/)"
docker build -t "${WEB_IMAGE}" web/

echo ">> saving images to tar"
docker save "${API_IMAGE}" -o "${API_TAR}"
docker save "${WEB_IMAGE}" -o "${WEB_TAR}"

echo ">> importing into local k3s containerd"
sudo k3s ctr images import "${API_TAR}"
sudo k3s ctr images import "${WEB_TAR}"

# Optional: push to other nodes passed as args (must be ssh-reachable).
for node in "$@"; do
    echo ">> shipping tars to ${node}"
    scp "${API_TAR}" "${WEB_TAR}" "${node}:/tmp/"
    echo ">> importing on ${node}"
    ssh "${node}" "sudo k3s ctr images import /tmp/${API_TAR} && sudo k3s ctr images import /tmp/${WEB_TAR} && rm -f /tmp/${API_TAR} /tmp/${WEB_TAR}"
done

echo ">> cleaning up local tars"
rm -f "${API_TAR}" "${WEB_TAR}"

echo ">> done. verify with: sudo k3s ctr images ls | grep anamnesis"
