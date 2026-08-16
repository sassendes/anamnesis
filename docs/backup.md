# Backup & DR (3-node HA)

Two independent layers, both off-box to MinIO:

1. **Logical**: nightly CronJob `k8s/backup/postgres-backup-cronjob.yaml`
   runs `pg_dump --format=custom | gzip`, encrypts with
   `openssl enc -aes-256-cbc -pbkdf2` (`backup-passphrase` in
   `anamnesis-db-secrets`) and writes `<date>.dump.enc` to the
   `anamnesis-backup-pvc` (50Gi). Retention: 14 days, auto-prune. The
   `s3` sidecar (minio/mc) mirrors `/backup` into the `backups`
   bucket of in-cluster MinIO (`k8s/backup/minio.yaml`), after waiting for
   the dump to appear, so it never races.
2. **Physical: CloudNativePG** (`k8s/postgres/cnpg-cluster.yaml`) - 3
   instances, synchronous replication, required anti-affinity. Every WAL
   segment is shipped by the operator's barman to the same MinIO
   (`s3://anamnesis/wals`, `cnpg-s3` secret), `retentionPolicy: 7d`.
   A `ScheduledBackup` (01:00 nightly) produces barman base backups → full
   point-in-time recovery.

## Backup inventory

| artifact | producer | where | used for |
|---|---|---|---|
| nightly `.dump.enc` | backup CronJob | backup PVC | logical restores, audit drill |
| barman base + WAL | CNPG operator | MinIO `anamnesis/wals` | PITR |
| weekly restore drill | `backup-restore-audit` CronJob | scratch DB | proves restore works |
| s3 offload | backup CronJob (mc mirror) | MinIO `backups/` bucket | off-box copy |
| etcd snapshots | k3s (03:00, ×14) | each node `/var/lib/rancher/k3s/server/db/snapshots` | control-plane recovery |

## Weekly restore drill (automated)

```bash
kubectl -n anamnesis get cronjob backup-restore-audit
```

The job decrypts the newest dump, restores into `anamnesis_audit_$$`,
counts `patients`, drops the scratch db, and fails if the row count is 0.

## Manual logical restore into the cluster

1. Take a safety dump first.
2. `kubectl -n anamnesis exec anamnesis-1 -- psql -U postgres -c 'drop database anamnesis;'`
3. Decrypt and stream:
   `openssl enc -d -aes-256-cbc -pbkdf2 -pass pass:<passphrase> < backup.dump.enc | kubectl -n anamnesis exec -i anamnesis-1 -- psql -U postgres -d anamnesis`
4. `readyz` goes green; Grafana shows the history again.

## Point-in-time recovery (barman)

CloudNativePG restore - the operator runs barman under the hood, and the
point-in-time target comes from the MinIO WAL archive. Fresh PITR cluster:

```bash
kubectl cnpg backup anamnesis --method barmanObjectStore   # manual base
kubectl cnpg restore anamnesis-pitr --source anamnesis \
  --method barmanObjectStore --target-time "2026-08-09 03:12:00"
```

Point the app `database-url` at the new `-rw` service until data is verified.

Migration 0003 is idempotent (`IF NOT EXISTS`), so a second apply is safe.

## Control plane recovery (etcd)

k3s snapshots run nightly (03:00, 14 kept) on every server node:

```bash
ssh root@k3s1 'k3s etcd-snapshot ls --dir /var/backups'
# restore on a broken node:
ssh root@k3s2 'k3s server --cluster-reset --cluster-reset-restore-path=/var/backups/<snapshot>'
```

## Notes

- Backing up is not the same as proving it: the weekly audit job exists
  for that. Nobody gets a pass until a restore has run this week.
- Restoring overwrites current data; take a fresh dump first.
- Rotating `backup-passphrase` only affects dumps written after rotation.