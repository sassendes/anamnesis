#!/bin/sh
# install ingress-nginx on k3s (the default traefik is too small for the
# blackbox/loki/grafana path length checks; this is a full-featured proxy)
# usage: as root on the server node
set -eu

VERSION="${1:-controller-v1.12.0}"

if ! command -v kubectl >/dev/null; then
  echo "kubectl not found - not a k3s server?" >&2
  exit 1
fi

kubectl apply -f "https://raw.githubusercontent.com/kubernetes/ingress-nginx/$VERSION/deploy/static/provider/kind/deploy.yaml"

kubectl -n ingress-nginx rollout status deployment/ingress-nginx-controller --timeout=120s

kubectl -n ingress-nginx get svc