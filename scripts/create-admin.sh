#!/usr/bin/env bash
set -euo pipefail
HOSPITAL_ID="${1:?need hospital_id UUID}"
USERNAME="${2:?need username}"
HASH="${3:?need argon2 hash - gen at https://emn178.github.io/online-tools/argon2/}"
kubectl exec -i anamnesis-1 -- psql -U postgres -d anamnesis << EOF
INSERT INTO staff (hospital_id, username, name, role_title, roles, password_hash)
VALUES ('${HOSPITAL_ID}', '${USERNAME}', 'Administrator', 'Administrator', ARRAY['admin','doctor'], '${HASH}')
ON CONFLICT (hospital_id, username) DO UPDATE SET password_hash = EXCLUDED.password_hash;
SELECT username, hospital_id FROM staff WHERE username = '${USERNAME}';
EOF
