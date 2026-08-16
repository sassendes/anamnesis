-- Least-privilege runtime grants for anamnesis_app.
--
-- The API connects as anamnesis_app, a non-superuser, which is what makes the
-- RLS policies in 0001 actually enforced (a superuser or table owner would
-- bypass them). Row visibility stays with those policies; this file only hands
-- the role the verbs it needs. No DELETE is granted: the API never hard-deletes.

GRANT SELECT, INSERT, UPDATE ON ALL TABLES IN SCHEMA public TO anamnesis_app;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO anamnesis_app;

-- Anything the owner creates later (e.g. monthly partitions) is reachable
-- through its parent table already; set defaults too for direct access.
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE ON TABLES TO anamnesis_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO anamnesis_app;

-- The audit trail is written only by the SECURITY DEFINER trigger. The app can
-- read its own hospital's rows (via RLS) but must never mutate the trail.
REVOKE INSERT, UPDATE, DELETE ON app_audit FROM anamnesis_app;

-- Migration bookkeeping is not the app's business.
REVOKE ALL ON _sqlx_migrations FROM anamnesis_app;

-- Tenant-context helpers used by nearly every query, plus the reporting views.
GRANT USAGE ON SCHEMA app TO anamnesis_app;
GRANT EXECUTE ON FUNCTION app.current_hospital_id() TO anamnesis_app;
GRANT EXECUTE ON FUNCTION app.current_staff_id() TO anamnesis_app;
GRANT SELECT ON app.open_admissions TO anamnesis_app;
GRANT SELECT ON app.daily_census TO anamnesis_app;
