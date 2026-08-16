# Secrets & SOPS

The demo ships with plaintext secrets in this directory so `kubectl apply -k k8s/`
just works on a fresh k3s box. Before any production use, move them into SOPS.

## What lives here

| file | contains |
|---|---|
| `anamnesis-secrets.yaml` | API database URL, JWT secret |
| `anamnesis-db-secrets.yaml` | postgres superuser + app password, backup passphrase |

All secrets default to `root` (JWT secret, DB passwords, backup passphrase) so the demo is uniform. Everything is demo-grade.

## Moving to encrypted secrets (age keys)

Inside the container once-off for keys; everything below happens outside it:

1. Generate an age keypair (on your laptop):

   ```bash
   age-keygen -o ~/.config/sops/age/keys.txt
   age-keygen -y ~/.config/sops/age/keys.txt   # prints the public key
   ```

2. Put the public key into `.sops.yaml` at the repo root (replace the
   `age1...` placeholder). The rule already targets `k8s/secrets/*.yaml`.

3. Encrypt, commit only the encrypted copy, drop the plaintext one:

   ```bash
   sops -e k8s/secrets/anamnesis-secrets.yaml > k8s/secrets/anamnesis-secrets.enc.yaml
   rm k8s/secrets/anamnesis-secrets.yaml
   git add k8s/secrets/anamnesis-secrets.enc.yaml
   ```

4. Decrypt at deploy time:

   ```bash
   sops -d k8s/secrets/anamnesis-secrets.enc.yaml | kubectl apply -f -
   ```

   Or wire kustomize to do it with the
   [sops KRM plugin](https://github.com/viadot-ai/kustomize-sops) -
   then a plain `kubectl apply -k k8s/` decrypts on the fly.

## Rotation checklist

- `jwt-secret`: restart the app pod, relogin all users.
- DB passwords: ALTER ROLE ... PASSWORD, update secrets, restart postgres.
- `backup-passphrase`: re-encrypt the next nightly dump; old dumps stay
  readable with the old passphrase until they expire (14d retention).