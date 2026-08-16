#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:?usage: offload-backup.sh DEST_DIR}"
DEST="$(realpath "$DEST")"
mkdir -p "$DEST"

NS=anamnesis
cat <<EOF | kubectl -n "$NS" apply -f -
apiVersion: v1
kind: Pod
metadata:
  name: backup-offload
spec:
  restartPolicy: Never
  containers:
    - name: offload
      image: busybox
      command: ["sh", "-c", "tar czf - -C /backup . > /out.tar.gz"]
      volumeMounts:
        - name: backup
          mountPath: /backup
        - name: out
          mountPath: /out.tar.gz
          subPath: out.tar.gz
  volumes:
    - name: backup
      persistentVolumeClaim:
        claimName: anamnesis-backup-pvc
    - name: out
      emptyDir: {}
EOF
trap 'kubectl -n "$NS" delete pod backup-offload --ignore-not-found >/dev/null' EXIT
kubectl -n "$NS" wait --for=condition=Ready pod/backup-offload --timeout=2m 2>/dev/null
kubectl -n "$NS" cp backup-offload:/tmp/out.tar.gz "$DEST/backups.tar.gz"
kubectl -n "$NS" delete pod backup-offload --ignore-not-found >/dev/null
trap - EXIT
tar tzf "$DEST/backups.tar.gz" >/dev/null && echo "offload OK -> $DEST/backups.tar.gz"
