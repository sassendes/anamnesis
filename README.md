# Anamnesis

Multi-tenant clinical records API in Rust, with the operational stack around it that a real deployment would want: 3-node k3s with embedded etcd, HA PostgreSQL, TLS, alerts, backups and continuous delivery. The idea is that the boring infrastructure is all real and demoable, not that the codebase is huge.

This is a demo, not production software. It is built to look, feel and behave like the real thing: tenants you cannot cross, passwords you cannot harvest, backups you can actually restore, an upgrade path that runs in CI. The gaps are documented up front (see "Known limitations") instead of hidden.

## What's inside

- Rust API (axum, sqlx): patients, wards, admissions, labs, prescriptions, billing, KPIs, audit trail.
- Tenant isolation straight in the database: every row is scoped to a hospital, RLS enforces it, the API can't see across.
- JWT auth with argon2-hashed passwords, role-based endpoint checks, per-IP rate limiting and per-user lockout. Optional OIDC/SSO (RS256 JWKS verification, no auto-provisioning).
- Outbox for side effects: writes land in the outbox in the same transaction, a dispatcher publishes NATS events when configured.
- Webhooks for sensitive events (account lockouts, unprovisioned SSO logins, invoice charges).
- A React SPA (`web/`) for login and dashboards, served by nginx.

## Layout

| Path | What lives there |
|---|---|
| `app/` | Rust API, migrations, tests, admin binaries |
| `app/src/routes/` | one axum router file per domain |
| `web/` | Vite + React SPA, nginx container |
| `k8s/` | kustomize base for the 3-node HA deployment |
| `ansible/` | VM bootstrap for the k3s cluster |
| `scripts/` | smoke, provisioning, k6 load, cluster addons, backup offload |
| `docs/` | architecture, runbook, backup and DR |
| `.github/workflows/` | CI, release, load test, per-PR previews |

## Quick start (local, Docker)

Prerequisites: Docker with compose, Make, and a Rust toolchain if you want to use the developer commands below.

```bash
docker compose up -d --build
make provision-admin \
  HOSPITAL_ID=HCP-01 \
  USERNAME=sysadmin PASSWORD='Doctor123!'
```

The API answers at `http://localhost:8080` once the stack is up (`curl http://localhost:8080/api/v1/healthz`). The SPA in `web/` is built as its own nginx container and ships with the k8s deployment (`anamnesis-web`); the local compose stack is API + postgres only.

Why two connect strings? `docker compose up` runs migrations and seeding as the database superuser, then starts the API as the least-privileged `anamnesis_app` role, so row-level security is actually enforced (a superuser would bypass it). `make provision-admin` also connects as the superuser: provisioning resolves a hospital by code across tenants and inserts staff, which RLS would otherwise block. Both default to `postgres:root` on `localhost:5432`.

Two demo hospitals are seeded (HCP-01 and CPV-02) with wards, beds, medications and lab panels. There are no default logins: provision one. `HOSPITAL_ID` accepts either the hospital code (`HCP-01`) or its UUID.

If you change roles or re-bootstrap, run `docker compose down -v` first so a stale volume doesn't keep an old role definition around.

## Development

```bash
make ci              # fmt + clippy -D warnings + tests + release build
make test            # unit and router tests
make smoke           # end-to-end against a throwaway postgres (scripts/smoke.sh)
make provision-admin HOSPITAL_ID=<uuid|code> USERNAME=<u> PASSWORD=<p>
make manifests-dryrun  # kubectl apply --dry-run=client -k k8s/
make install-git-hooks # pre-commit: fmt, clippy, tests
```

## API

Everything is under `/api/v1`, one router file per domain in `app/src/routes/`:

| Area | Endpoints |
|---|---|
| Health | `GET /healthz`, `/livez`, `/readyz`, `/status` |
| Auth | `POST /auth/login`, `POST /auth/oidc`, `GET /auth/me` |
| Patients | `GET|POST /patients`, `GET|PATCH /patients/{id}`, `GET|POST /patients/{id}/vitals`, `POST /patients/{id}/allergies`, `POST /patients/{id}/diagnoses` |
| Clinical | `POST /patients/{id}/admissions`, `POST /admissions/{id}/discharge`, `POST /patients/{id}/prescriptions`, `POST /patients/{id}/lab-orders`, `POST /lab-results` |
| Labs & drugs | `GET /diagnostics/codes`, `GET /medications`, `GET /labs/orders`, `GET /labs/results/{order_id}`, `GET /results` |
| Billing | `GET /invoices/{id}`, `POST /visits/{id}/invoice`, `POST /invoices/{id}/charge` |
| Wards & KPIs | `GET /wards`, `GET /dashboard/stats` |
| Audit | `GET /audit` (admin only) |

Health endpoints are what the probes in `k8s/app/deployment.yaml` hit, and `/api/v1/metrics` feeds Prometheus. All configuration comes from environment variables; the full list is in `.env.example` (database URL, JWT secret and TTL, listen addresses, NATS, OIDC issuer and client, webhook URL, OTLP endpoint).

## Testing & CI

GitHub Actions runs on every push and PR:

- `ci.yml`: fmt, clippy with `-D warnings`, release build, unit and router tests, `cargo-audit`, trivy (filesystem and secret scan), gitleaks, kubeconform and a kubectl dry-run of the whole kustomize tree.
- The tenant-isolation test runs against a real postgres (superuser runs the migrations, then the app role with RLS on), so crossing a hospital boundary fails the build.
- `load.yml`: k6 ramp test, weekly on schedule.
- `preview.yml`: per-PR image pushed to GHCR, Argo CD ApplicationSet stands up a preview namespace on the demo cluster.
- `release.yml`: `v*` tags build multi-arch images (amd64/arm64), sign them with cosign, attach an SBOM, then open a PR that bumps the image tag in `k8s/app/deployment.yaml`.

## Deploying (3 VMs)

```bash
# 1. put the repo on /opt/anamnesis on each host
# 2. run once as root:
ansible-playbook -i ansible/hosts ansible/k3s-demo.yml
```

That boots the cluster, installs cert-manager, ingress-nginx, CloudNativePG, the app, the observability stack, TLS and backups, and waits for the database cluster to be Ready. Details are in `docs/runbook.md`, including the turn-the-node-off failover drill and what the security layers actually do.

## Operational story

Everything below is real, in this repo:

- 3-node k3s with embedded etcd, bootstrapped by Ansible, ufw + fail2ban + unattended-upgrades on the box.
- CloudNativePG 3-instance cluster with synchronous replication, auto-failover and WAL archiving to in-cluster MinIO.
- Nightly encrypted logical backups, weekly automated restore drill, off-box S3 mirror, etcd snapshots.
- Prometheus with SLO alerts, Alertmanager, Grafana, Loki/promtail, Tempo via OTLP.
- cert-manager private CA, Kyverno policies, network policies, per-workload service accounts.
- Argo CD with per-PR preview environments, weekly k6 load test, monthly chaos drill.

The full inventory is in `docs/architecture.md`; the retention plan and restore procedures are in `docs/backup.md`.

## Known limitations

- No refresh tokens, MFA or password reset flow.
- Plain LIMIT/OFFSET pagination, no cursors.
- No automated tests for the SQL trigger bodies; the RLS and tenant paths are covered by the CI integration test.
- No NATS consumers yet: the demo emits events, nothing subscribes.

These are deliberately out of scope so the repo demonstrates a complete, real deployment without pretending to be a product. They are also where contributions are welcome.

## Demo-grade secrets (read this before "promoting")

Every credential in the repo defaults to `root`: the JWT secret, the database passwords, the backup passphrase. That is on purpose, so `docker compose up` and `kubectl apply -k k8s/` work on a fresh machine with zero setup. Before anything touches real data, rotate all of them and move the k8s secrets into SOPS: the rule in `.sops.yaml` and the step-by-step in `k8s/secrets/README.md` already exist.

Same story for the TLS: cert-manager uses a private CA created in the cluster (`anamnesis-ca`), which is the right shape for an on-prem demo and wrong shape for public traffic. Swap the ClusterIssuer, not the pattern.

## License

MIT, see [LICENSE](LICENSE). This project is a demo: don't put real patient data in it.