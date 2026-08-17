# Anamnesis

Multi-tenant clinical records API in Rust, with the operational stack a real deployment needs around it: a 3-node k3s cluster with embedded etcd, HA PostgreSQL, TLS, metrics/logs/traces, and backups. The point is that the boring infrastructure is all real and demoable, not that the codebase is huge.

This is a demo, not production software. It is built to look, feel and behave like the real thing: tenants you cannot cross, passwords you cannot harvest, an audit trail on every write. It is LAN-only by design, the way a real hospital records system is, reached from inside the network rather than exposed to the internet. Gaps are documented up front (see "Known limitations") instead of hidden.

## What's inside

- Rust API (axum 0.8, sqlx 0.8): patients, wards, admissions, labs, prescriptions, billing, KPIs, audit trail.
- Tenant isolation in the database: every row is scoped to a hospital, Row-Level Security enforces it, the API can't see across tenants. Enforced because the API connects as a least-privileged role that cannot bypass RLS.
- JWT auth (HS256) with argon2id-hashed passwords, role-based endpoint checks, per-IP login rate limiting and per-user lockout. Optional OIDC/SSO (RS256 JWKS verification, no auto-provisioning).
- Outbox pattern: writes land in the outbox in the same transaction, a dispatcher publishes NATS events when configured.
- Webhooks for sensitive events (account lockouts, unprovisioned SSO logins, invoice charges).
- A React SPA (`web/`) served by nginx: hospital-dropdown login, dashboards, and patient create/edit.

## Layout

| Path | What lives there |
|---|---|
| `app/` | Rust API, migrations, tests, admin binaries |
| `app/src/routes/` | one axum router file per domain |
| `web/` | Vite + React SPA, nginx container |
| `k8s/` | kustomize base for the 3-node HA deployment |
| `scripts/` | cluster setup, image build/deploy, admin bootstrap, smoke, seed, backup offload |
| `docs/` | architecture |

## Deploying to the cluster (3 nodes)

The real deploy path is script-driven. On the first control-plane node (cp1):

```bash
# 1. bring up the k3s cluster (run init on cp1, join on the other two)
./scripts/setup-cluster.sh init                       # cp1: prints a join token
./scripts/setup-cluster.sh join <CP1_IP> <TOKEN>      # on each other node
./scripts/setup-cluster.sh operators                  # cp1, once all nodes joined: cnpg + cert-manager

# 2. build the images and distribute them to every node
./scripts/deploy-cp1.sh                                # cp1: build + import, then serves the tars
./scripts/deploy-other-nodes.sh <CP1_IP>              # on each other node: pull + import

# 3. apply the manifests
k3s kubectl apply -k k8s/

# 4. bootstrap an admin login
./scripts/bootstrap-login.sh
```

Images use `imagePullPolicy: Never`, so each node needs the image imported locally (that's what the deploy scripts handle). After `apply -k`, the app comes up behind Traefik on `anamnesis.local` (TLS from the in-cluster `anamnesis-ca` CA, so add that CA to your trust store or expect a browser warning).

Two demo hospitals are seeded (HCP-01 and CPV-02) with wards, beds, medications and lab panels. There are no default logins: `bootstrap-login.sh` creates an `admin` account (password `Doctor123`) in the first hospital. `scripts/seed-patients.sh <count>` bulk-loads patients across both hospitals.

## Quick start (local, Docker)

For working on the API without a cluster:

```bash
docker compose up -d --build
make provision-admin HOSPITAL_ID=HCP-01 USERNAME=sysadmin PASSWORD='Doctor123!'
```

The API answers at `http://localhost:8080` (`curl http://localhost:8080/api/v1/healthz`). Compose runs migrations and seeding as the database superuser, then starts the API as the least-privileged `anamnesis_app` role so RLS is actually enforced. The SPA is a k8s-only container; local compose is API + postgres only.

## Development

```bash
make ci              # fmt-check + clippy -D warnings + tests + release build
make test            # unit and router tests
make smoke           # end-to-end against a throwaway postgres (scripts/smoke.sh)
make provision-admin HOSPITAL_ID=<uuid|code> USERNAME=<u> PASSWORD=<p>
make manifests-dryrun  # kubectl apply --dry-run=client -k k8s/
```

CI is a local gate (`scripts/ci.sh` / `make ci`): fmt, clippy with `-D warnings`, tests, release build, and a kustomize dry-run. There is no hosted CI in this repo.

## API

Everything is under `/api/v1`, one router file per domain in `app/src/routes/`:

| Area | Endpoints |
|---|---|
| Health | `GET /healthz`, `/livez`, `/readyz`, `/status`, `/metrics` |
| Auth | `POST /auth/login`, `POST /auth/oidc`, `GET /auth/me` |
| Patients | `GET\|POST /patients`, `GET\|PATCH /patients/{id}`, `GET\|POST /patients/{id}/vitals`, `POST /patients/{id}/allergies`, `POST /patients/{id}/diagnoses` |
| Clinical | `POST /patients/{id}/admissions`, `POST /admissions/{id}/discharge`, `POST /patients/{id}/prescriptions`, `POST /patients/{id}/lab-orders`, `POST /lab-results` |
| Labs & drugs | `GET /diagnostics/codes`, `GET /medications`, `GET /labs/orders`, `GET /labs/results/{order_id}`, `GET /results` |
| Billing | `GET /invoices/{id}`, `POST /visits/{id}/invoice`, `POST /invoices/{id}/charge` |
| Wards & KPIs | `GET /wards`, `GET /dashboard/stats` |
| Audit | `GET /audit` |

Health endpoints back the probes in `k8s/app/deployment.yaml`, and `/api/v1/metrics` feeds Prometheus. All config is via environment variables; the full list is in `.env.example`.

## Operational story

Everything below is real, in this repo:

- 3-node k3s with embedded etcd, host-gw flannel, brought up by `scripts/setup-cluster.sh`.
- CloudNativePG 3-instance PostgreSQL cluster with synchronous replication, auto-failover, and WAL archiving to in-cluster MinIO.
- Nightly encrypted logical backups + weekly restore-audit + off-cluster S3 mirror; k3s etcd snapshots.
- Prometheus with SLO alerts, Alertmanager, Grafana, Loki + promtail, Tempo via OTLP. Four live dashboards.
- cert-manager private CA, Kyverno policies, NetworkPolicies (default-deny), per-workload ServiceAccounts.

The full inventory is in `docs/architecture.md`.

## Known limitations

- No refresh tokens, MFA, or password reset flow.
- Plain LIMIT/OFFSET pagination, no cursors.
- No automated tests for the SQL trigger bodies; RLS and tenant paths are covered by the integration test.


## Demo-grade secrets (read before "promoting")

Every credential in the repo defaults to `root`: the JWT secret, the database passwords, the backup passphrase. That is on purpose so a fresh `apply -k k8s/` works with zero setup. Before anything touches real data, rotate all of them and move the k8s secrets into SOPS (see `k8s/secrets/README.md`). Same for TLS: cert-manager uses a private in-cluster CA (`anamnesis-ca`), which is right for an on-prem/LAN demo and wrong for public traffic. Swap the ClusterIssuer, not the pattern.

## License

MIT, see [LICENSE](LICENSE). This project is a demo: don't put real patient data in it.
