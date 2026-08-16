#!/bin/sh
set -eu

CLUSTER_WAIT_SECONDS=120

kubectl apply -f https://github.com/cert-manager/cert-manager/releases/download/v1.17.0/cert-manager.yaml
kubectl wait --for=condition=Available deployment/cert-manager-webhook -n cert-manager --timeout=${CLUSTER_WAIT_SECONDS}s
kubectl apply -f https://github.com/cloudnative-pg/cloudnative-pg/releases/download/v1.26.0/cnpg-1.26.0.yaml
kubectl wait --for=condition=Available deployment/cnpg-controller-manager -n cnpg-system --timeout=${CLUSTER_WAIT_SECONDS}s

sh /opt/anamnesis/scripts/install-ingress-nginx.sh

kubectl apply -f /opt/anamnesis/k8s/security/clusterissuer.yaml