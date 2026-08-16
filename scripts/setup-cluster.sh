#!/usr/bin/env bash
# Cluster prerequisites for anamnesis. Run BEFORE kubectl apply -k k8s/.
#
# Usage:
#   On the FIRST control-plane node:   ./setup-cluster.sh init
#   On the OTHER two nodes:            ./setup-cluster.sh join <FIRST_NODE_IP> <TOKEN>
#   After all nodes joined (on node1): ./setup-cluster.sh operators
#
# host-gw flannel backend is baked in from the start to avoid the VXLAN
# cross-node TCP failures that break CloudNativePG on virtualized nodes.
set -euo pipefail

CNPG_VERSION="1.29.1"
CERT_MANAGER_VERSION="v1.18.2"

case "${1:-}" in
  init)
    echo ">> installing k3s (first control-plane, host-gw)..."
    curl -sfL https://get.k3s.io | sh -s - server \
      --cluster-init \
      --flannel-backend=host-gw
    echo ">> node token (use this to join the other nodes):"
    cat /var/lib/rancher/k3s/server/node-token
    ;;

  join)
    FIRST_IP="${2:?need first node IP}"
    TOKEN="${3:?need token}"
    echo ">> joining cluster at ${FIRST_IP} (host-gw)..."
    curl -sfL https://get.k3s.io | sh -s - server \
      --server "https://${FIRST_IP}:6443" \
      --token "${TOKEN}" \
      --flannel-backend=host-gw
    ;;

  operators)
    echo ">> installing CloudNativePG operator ${CNPG_VERSION}..."
    kubectl apply --server-side --force-conflicts \
      -f "https://raw.githubusercontent.com/cloudnative-pg/cloudnative-pg/release-${CNPG_VERSION%.*}/releases/cnpg-${CNPG_VERSION}.yaml"
    kubectl rollout status deployment -n cnpg-system cnpg-controller-manager --timeout=180s

    echo ">> installing cert-manager ${CERT_MANAGER_VERSION}..."
    kubectl apply -f "https://github.com/cert-manager/cert-manager/releases/download/${CERT_MANAGER_VERSION}/cert-manager.yaml"
    kubectl -n cert-manager rollout status deploy/cert-manager --timeout=180s
    kubectl -n cert-manager rollout status deploy/cert-manager-webhook --timeout=180s
    kubectl -n cert-manager rollout status deploy/cert-manager-cainjector --timeout=180s

    echo ">> operators ready. now build+import images, then: kubectl apply -k k8s/"
    ;;


  *)
    echo "usage:"
    echo "  ./setup-cluster.sh init                          (first node)"
    echo "  ./setup-cluster.sh join <FIRST_NODE_IP> <TOKEN>  (other nodes)"
    echo "  ./setup-cluster.sh operators                     (node1, after all joined)"
    exit 1
    ;;
esac
