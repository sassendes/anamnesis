#!/bin/sh
# ship node journald (k3s, system services) to Loki via promtail
# install as a systemd service: /etc/systemd/system/promtail-host.service
set -eu

LOKI_URL="${LOKI_URL:-http://10.0.0.10:3100}"
PROMTAIL_BIN="${PROMTAIL_BIN:-/usr/local/bin/promtail}"
CONFIG=/etc/promtail-host.yml

cat > "$CONFIG" <<EOF
server:
  http_listen_port: 9080
clients:
  - url: $LOKI_URL/loki/api/v1/push
scrape_configs:
  - job_name: journald
    journal:
      max_age: 12h
      labels:
        job: systemd-journal
    relabel_configs:
      - source_labels: ["__journal__systemd_unit"]
        target_label: unit
      - source_labels: ["__journal_hostname"]
        target_label: host
EOF

if [ ! -x "$PROMTAIL_BIN" ]; then
  echo "downloading promtail binary..."
  V=$(curl -sL https://api.github.com/repos/grafana/loki/releases/latest | sed -n 's/.*"tag_name": "\(v[^"]*\)".*/\1/p')
  curl -sL "https://github.com/grafana/loki/releases/download/$V/promtail-linux-amd64.zip" -o /tmp/promtail.zip
  python3 - <<'PY'
import zipfile
zipfile.ZipFile('/tmp/promtail.zip').extract('promtail-linux-amd64', '/tmp')
PY
  mv /tmp/promtail-linux-amd64 "$PROMTAIL_BIN"
  chmod +x "$PROMTAIL_BIN"
fi

exec "$PROMTAIL_BIN" -config.file="$CONFIG"