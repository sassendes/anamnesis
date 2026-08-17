#!/usr/bin/env bash
# Bootstrap an admin login for Anamnesis.
# Login after running:
#   hospital: Hopital Central de Plaisir
#   username: admin
#   password: Doctor123
set -euo pipefail

HOSPITAL_ID="11111111-1111-1111-1111-111111111111"
USERNAME="admin"
HASH='$argon2id$v=19$m=65536,t=3,p=1$3Azr3yjt/rPwD8odBvJXLg$KEN5bhl7GJIWJQzShLSyMRVJbAKKS4oM1tI/aURfXcg'
ROLES="admin,doctor,nurse,pharmacist"

echo ">> creating/resetting '${USERNAME}' in hospital ${HOSPITAL_ID}"

k3s kubectl exec -i anamnesis-1 -- psql -U postgres -d anamnesis <<SQL
INSERT INTO staff (hospital_id, username, name, role_title, roles, active, password_hash)
VALUES (
    '${HOSPITAL_ID}',
    '${USERNAME}',
    'Administrator',
    'Administrator',
    string_to_array('${ROLES}', ',')::text[],
    true,
    '${HASH}'
)
ON CONFLICT (hospital_id, username) DO UPDATE
SET password_hash = EXCLUDED.password_hash,
    roles = EXCLUDED.roles,
    active = true,
    updated_at = now();

SELECT username, roles, active FROM staff WHERE username = '${USERNAME}';
SQL

echo ""
echo ">> done. log in with:"
echo "   hospital: Hopital Central de Plaisir"
echo "   username: admin"
echo "   password: Doctor123"
