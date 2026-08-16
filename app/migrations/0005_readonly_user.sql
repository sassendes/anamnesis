-- read-only reporting role for Grafana dashboards.
-- BYPASSRLS so cross-tenant reporting queries see all hospitals
-- (the app role stays tenant-scoped; this is admin read-only reporting).
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'anamnesis_readonly') THEN
        CREATE ROLE anamnesis_readonly LOGIN PASSWORD 'readonly';
    END IF;
END $$;

ALTER ROLE anamnesis_readonly LOGIN PASSWORD 'readonly';
ALTER ROLE anamnesis_readonly BYPASSRLS;
GRANT USAGE ON SCHEMA public TO anamnesis_readonly;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO anamnesis_readonly;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO anamnesis_readonly;
