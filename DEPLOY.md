# Anamnesis — deploy runbook (k3s, default namespace)

## Prereqs on the cluster
- 3-node k3s with embedded etcd (done)
- CNPG operator installed (done)
- Traefik ingress (ships with k3s)
- metrics-server (ships with k3s — HPA needs it)

## 1. Build the two images (on a machine with docker), import to k3s

API image:
```
cd app
docker build -t anamnesis:local .
docker save anamnesis:local -o anamnesis.tar
```
Web image:
```
cd ../web
docker build -t anamnesis-web:local .
docker save anamnesis-web:local -o anamnesis-web.tar
```
Copy both tars to EVERY node (pods can land on any of the 3), then on each node:
```
k3s ctr images import anamnesis.tar
k3s ctr images import anamnesis-web.tar
```

## 2. Apply everything (core bundle = no kyverno/argocd/service-monitor)
```
kubectl apply -k k8s/
```
Order is handled by kustomize. The migrate Job runs the schema; the API pods
CrashLoop-retry until the DB + migration are ready, then go green. That's expected.

## 3. Verify each subsystem

### Postgres HA (3 instances, one per node)
```
kubectl get pods -l cnpg.io/cluster=anamnesis -o wide      # 3 pods on 3 nodes
kubectl get cluster anamnesis                              # Cluster in healthy state, 1 primary 2 replicas
kubectl cnpg status anamnesis                              # (plugin) primary, replicas, no lag
```

### App talking to DB
```
kubectl get pods -l app=anamnesis-api                     # Running, ready
kubectl logs deploy/anamnesis-api | tail                  # "anamnesis serving"
kubectl exec deploy/anamnesis-api -- wget -qO- localhost:8080/api/v1/_health
```

### Migration ran
```
kubectl get job anamnesis-migrate                         # Complete
```

### Backups
On-demand backup to test the whole path:
```
kubectl cnpg backup anamnesis                             # or: create a Backup CR
kubectl get backup                                        # phase: completed
```
Scheduled backup fires daily at 01:00:00 (6-field cron).

### Failover (prove real HA)
```
kubectl delete pod <primary-pod>                          # cnpg promotes a replica
kubectl get cluster anamnesis -w                          # new primary elected, stays healthy
```

### Observability
```
kubectl get pods -l app=prometheus,app=grafana,app=loki,app=tempo
# grafana.anamnesis.local via traefik (add to /etc/hosts -> node IP)
```

### Autoscaling
```
kubectl get hpa anamnesis-api                             # shows current/target, scales 2..8
```

## Namespaces
Everything is in `default`. The bundled cnpg cluster is named `anamnesis`
(service `anamnesis-rw`), separate from any other postgres you run.

## The full bundle
`kustomization-full.yaml` additionally includes kyverno policies, argo appset,
and a Prometheus-Operator ServiceMonitor. Only use it after installing those
operators (kyverno, prometheus-operator, argocd). The default `kustomization.yaml`
is the operator-free bundle that runs on a plain k3s + cnpg box.
