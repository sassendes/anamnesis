#!/bin/sh
# install Chaos Mesh and a weekly resilience drill targeting anamnesis-api
# run as root on the k3s server node
set -eu

if ! command -v kubectl >/dev/null; then
  echo "kubectl not found" >&2
  exit 1
fi

kubectl apply -f https://mirrors.chaos-mesh.org/v2.6.3/chaos-mesh-v2.6.3.yaml

kubectl -n chaos-mesh rollout status deployment/chaos-controller-manager --timeout=180s

kubectl apply -f - <<'EOF'
apiVersion: chaos-mesh.org/v1alpha1
kind: Workflow
metadata:
  name: anamnesis-weekly-drill
  namespace: default
spec:
  entry: main
  templates:
    - name: main
      type: Serial
      children:
        - pod-kill
        - latency
    - name: pod-kill
      type: PodChaos
      deadline: 30s
      podChaos:
        action: pod-kill
        mode: one
        selector:
          labelSelectors:
            app: anamnesis-api
    - name: latency
      type: NetworkChaos
      deadline: 5m
      networkChaos:
        action: delay
        mode: one
        selector:
          labelSelectors:
            app: anamnesis-api
        delay:
          latency: 300ms
        jitter: 80ms
EOF