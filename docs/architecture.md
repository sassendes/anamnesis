# Anamnesis - architecture

A multi-tenant hospital API demo: Rust (axum) + PostgreSQL, hardened,
observable, and deployable with a single `kubectl apply -k k8s/`.

## Runtime shape

- `app/` - axum 0.8 API, sqlx 0.8, one crate with routers under `app/src/routes/`.
- `web/` - Vite + React SPA served by nginx, proxying `/api` to the API.
- `k8s/` - kustomize base for a 3-node k3s HA deployment.
- `ansible/` - VM bootstrap for 3 k3s servers (embedded etcd, ufw, fail2ban,
  unattended-upgrades).
- `.github/workflows/` - CI (fmt/clippy/test/migrate/e2e/audit/trivy/gitleaks/
  kubeconform), release (multiarch GHCR + cosign + SBOM), load test (k6),
  preview (per-PR image + Argo CD comment).

## The one idea that matters: tenant isolation

Every row in every table belongs to one `hospital_id`. The app sets a
session variable per request (`app.set_config('tenancy.hospital_id', ...)`);
Row-Level Security policies on every table read that session and hide
everything else. The DB is the security boundary, not the API:

- `app.current_hospital_id()` - session variable, nil-safe
- `ENABLE ROW LEVEL SECURITY` + `FORCE` on every tenant table
- unique indexes make the API fast; the trigger-based audit log sees all.

## Auth

- Argon2 (argon2id) password hashes with random salts, 30s active-cache plus a DB check on cache miss.
- JWT (HS256) signed with `jwt-secret`; token carries hospital + roles.
- Per-IP rate limit (20/min on login), per-user lockout with backoff.
- OIDC SSO optional: `ANAMNESIS_OIDC_ISSUER` + `ANAMNESIS_OIDC_CLIENT_ID`
  enable `POST /api/v1/auth/oidc`, which verifies an RS256 `id_token`
  against the provider JWKS and issues the same app JWT. Staff must be
  provisioned (username = the provider email) - no automatic onboarding.

## The outbox

Writes go into `outbox` (event_type + payload JSON) in the same
transaction as the business row. A dispatcher (`app/src/outbox.rs`)
claims rows with `FOR UPDATE SKIP LOCKED`, marks them delivered, and -
when `ANAMNESIS_NATS_URL` is set - publishes `anamnesis.events.<type>` to
NATS (JetStream mode; see `k8s/events/nats.yaml`). No NATS = the log line
is the event; nothing is lost from the DB.

## Webhooks & audit

- `app_audit` trigger captures INSERT/UPDATE/DELETE with old/new JSON;
  `GET /api/v1/audit` (admin) lists it, tenant-filtered.
- Sensitive actions (account lockouts, unprovisioned OIDC login, invoice
  charges) fire an HTTP webhook when `ANAMNESIS_WEBHOOK_URL` is set -
  plain HTTP POST, fire-and-forget (`app/src/webhooks.rs`).

## Tracing

`ANAMNESIS_OTEL_ENDPOINT` points at Tempo's OTLP/HTTP (`tempo:4318`).
When set, the app exports spans (Tower HTTP + axum) and Grafana gets a
Tempo datasource. Unset → the usual structured logs to stdout.

## Deployments

- `kubectl apply -k k8s/` brings up: namespace, API Deployment (HPA 2-8,
  spread across nodes), CloudNativePG PostgreSQL 17 cluster (3 instances,
  required anti-affinity, auto-failover, WAL archiving to MinIO via barman),
  nightly encrypted logical backup + weekly restore-audit + MinIO S3 mirror,
  NATS, Tempo, Prometheus (+blackbox, SLO alerts), Alertmanager, Grafana
  (API/Postgres dashboards), Loki + promtail, Postgres exporter,
  Kyverno policies, NetworkPolicies, per-workload ServiceAccounts,
  `anamnesis-web` (2 replicas) on `/` with `/api` proxied, cert-manager
  issuing TLS for the ingresses (ClusterIssuer `anamnesis-ca`).
- `docker-compose.yml` - the same stack on a laptop (db + migrate + API; the SPA is a k8s-only container).
- Argo CD: `k8s/argocd/application.yaml` (base) + `appset-preview.yaml`
  (per-PR preview namespaces via the pull-request generator).
- Secrets: plaintext for the demo (`root`/`root` everywhere), SOPS-ready
  (`.sops.yaml` + `k8s/secrets/README.md`).

## What is deliberately missing

- Refresh tokens / MFA / password reset flows.
- No pagination cursors (plain LIMIT + page for now).
- No tests on the SQL triggers (migrations only).
- NATS consumers for the events (demo emits, no subscriber yet).

These are the spots to grow. Everything else is the boring, working core.