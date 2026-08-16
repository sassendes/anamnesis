# Runbook for the 3-node HA demo cluster

Three k3s servers with embedded etcd, CloudNativePG for postgres, deployed
from the ansible playbook (`ansible/k3s-demo.yml`, hosts in `ansible/hosts`)
plus `kubectl apply -k k8s/` (or Argo CD via `k8s/argocd/`).

## First run on a fresh cluster

The playbook leaves you with a Ready database cluster and the app running,
but no staff yet (there are no default logins, same as local). Provision the
first admin against the cluster's read-write endpoint, then log in via the
SPA at `https://anamnesis.local` (cert from the in-cluster `anamnesis-ca`
CA, so the browser will complain until you add that CA to your trust store).

```bash
# from any machine that can reach the cluster
DATABASE_URL='postgres://postgres:root@<pg-rw-host>:5432/anamnesis?sslmode=disable' \
  make provision-admin HOSPITAL_ID=HCP-01 USERNAME=sysadmin PASSWORD='Doctor123!'
curl -k https://anamnesis.local/api/v1/healthz
```

## Daily triage

```bash
export KUBECONFIG=/etc/rancher/k3s/k3s.yaml

kubectl get nodes                                   # 3x Ready, control-plane
kubectl -n anamnesis get pods                       # api 2/2, web 2/2, pg 3/3
kubectl -n anamnesis get cluster,backup             # (g/cluster + backups)
kubectl -n anamnesis get hpa,svc,pvc,pdb            # scale + volumes + budgets
kubectl -n anamnesis get events --sort-by=.lastTimestamp | tail
kubectl -n anamnesis logs -l app=anamnesis-api --tail=100

# dashboards
kubectl -n anamnesis port-forward svc/grafana 3000:3000   # grafana.anamnesis.local
kubectl -n anamnesis port-forward svc/prometheus 9090:9090
kubectl -n anamnesis port-forward svc/anamnesis-web 8080:80   # SPA, also at https://anamnesis.local
```

## Control plane failover drill

```bash
# any single server can go down: etcd keeps quorum on the other two.
ssh root@k3s2 'systemctl stop k3s-server'
kubectl get nodes        # apiserver still answers (quorum on 2 nodes)
ssh root@k3s2 'systemctl start k3s-server'
```

## Postgres failover (CloudNativePG)

```bash
PRIMARY=$(kubectl -n anamnesis get po -l cnpg.io/role=primary -o name)
kubectl -n anamnesis delete $PRIMARY
kubectl -n anamnesis wait --for=jsonpath='{.status.phase}'=Ready cluster/anamnesis --timeout=180s
kubectl -n anamnesis get po -l cnpg.io/cluster=anamnesis
# app DSN points at anamnesis-rw, which now targets the new primary
```

## Restore from last night's backup

```bash
kubectl -n anamnesis get pods -l job-name=postgres-backup-*
```

(Full restore procedure lives in `docs/backup.md`.)

## New hospital "onboarding" (multi-tenant)

```bash
make provision-admin HOSPITAL_ID=<uuid> USERNAME=sysadmin PASSWORD=<pw>
```

## Upgrade (new migration)

```bash
kubectl -n anamnesis exec deploy/anamnesis-api -- \
  sh -c "ANAMNESIS_RUN_MIGRATIONS=1 anamnesis"
# or: make migrate -> push -> Argo CD syncs
```

## Capacity checks

`kubectl -n anamnesis get pvc` - every postgres instance has its own 20Gi
local-path PVC on the node it lands on; the backup PVC holds 14 days of
encrypted dumps (50Gi). Bump the cronjob retention if it fills.

## Throttling, lockout, deactivation

- Login rate-limited in-process: 20 req/min per client IP, 429 on exceed.
- Failed logins lock the username: 5 failures -> backoff that doubles
  (min 5s, capped). Clear it: `kubectl -n anamnesis rollout restart deploy/anamnesis-api`
- Deactivated staff get 401 on login and every authed call.
- Metrics: `login_attempts_total`, `outbox_backlog`,
  `http_request_duration_seconds`, `http_errors_total`.

## What the security toolbar actually does

- NetworkPolicies: k8s/security/network-policies.yaml - flannel does not
  enforce them; run a CNI that does (calico/cilium) for real isolation.
- Service accounts per workload: k8s/security/service-accounts.yaml.
- Kyverno: deny-exec (no `kubectl exec` shells), require image tags
  (no `:latest`).
- cert-manager: ClusterIssuer `anamnesis-ca` signs `anamnesis-tls` and
  `observability-tls`; ingress terminates TLS everywhere.

## Secrets cleanup before promoting

1. k8s/secrets/*.yaml: replace every default `root` value (JWT secret, DB
   passwords, backup passphrase).
2. .sops.yaml: replace the placeholder age key (see k8s/secrets/README.md).
3. kubectl -n anamnesis rollout restart deploy/anamnesis-api

## Moving parts (events, tracing, SSO, load, chaos, previews)

- NATS (nats:4222): outbox events land on `anamnesis.events.*` when the API
  env `ANAMNESIS_NATS_URL` is set.
- Tempo at tempo:3200; the app sends OTLP/HTTP when `ANAMNESIS_OTEL_ENDPOINT`
  is set (Grafana → trace datasource, query `{service.name="anamnesis"}`).
- OIDC: `ANAMNESIS_OIDC_ISSUER`/`ANAMNESIS_OIDC_CLIENT_ID`, staff username =
  provider email; RS256 JWKS verification, no auto-provisioning.
- Webhooks: `ANAMNESIS_WEBHOOK_URL` fires for login throttling,
  unprovisioned OIDC, invoice charges.
- Load test: `scripts/load/k6.js` (ramping 1→40 VUs, 2% error budget).
- Chaos drill: `k8s/chaos/install-chaos-mesh.sh` + the
  `anamnesis-weekly-drill` workflow kills API pods and adds 300ms latency.
- Preview envs: Argo ApplicationSet (pull-request generator) +
  `k8s/overlays/preview`; `pr-<n>.anamnesis.local` after CI,
  image `ghcr.io/<owner>/anamnesis:pr-<n>`.
- Backup drill: nightly CNPG barman base + WAL stream to MinIO (PITR)
  + weekly encrypted-logical restore audit.