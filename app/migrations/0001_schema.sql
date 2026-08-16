CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS btree_gist;
CREATE EXTENSION IF NOT EXISTS citext;
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;

-- pg_cron isn't in the stock postgres image; treat it as optional so local
-- migrations don't hard-fail. Where it's present (prod), scheduling still runs.
DO $cron$
BEGIN
    CREATE EXTENSION IF NOT EXISTS pg_cron;
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'pg_cron unavailable; scheduled jobs will not be registered';
END
$cron$;

DO $roles$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'anamnesis_app') THEN
        CREATE ROLE anamnesis_app LOGIN PASSWORD 'root';
    END IF;
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'anamnesis_migrator') THEN
        CREATE ROLE anamnesis_migrator LOGIN;
    END IF;
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'anamnesis_readonly') THEN
        CREATE ROLE anamnesis_readonly LOGIN;
    END IF;
END
$roles$;

CREATE SCHEMA IF NOT EXISTS app;

GRANT USAGE ON SCHEMA app, public TO anamnesis_app;

CREATE OR REPLACE FUNCTION app.current_hospital_id() RETURNS uuid
LANGUAGE sql STABLE
AS $$
    SELECT NULLIF(current_setting('app.hospital_id', true), '')::uuid
$$;

CREATE OR REPLACE FUNCTION app.current_staff_id() RETURNS text
LANGUAGE sql STABLE
AS $$
    SELECT NULLIF(current_setting('app.staff_id', true), '')
$$;

CREATE TABLE hospitals (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL,
    code text NOT NULL UNIQUE,
    country text NOT NULL DEFAULT 'FR',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE hospitals ENABLE ROW LEVEL SECURITY;
CREATE POLICY hospitals_tenant ON hospitals FOR ALL TO anamnesis_app
    USING (id = app.current_hospital_id())
    WITH CHECK (id = app.current_hospital_id());

CREATE TABLE departments (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    code text NOT NULL,
    name text NOT NULL,
    UNIQUE (hospital_id, code)
);

ALTER TABLE departments ENABLE ROW LEVEL SECURITY;
ALTER TABLE departments FORCE ROW LEVEL SECURITY;
CREATE POLICY departments_tenant ON departments FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE TABLE staff (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    username citext NOT NULL,
    name text NOT NULL,
    email citext,
    phone text,
    role_title text NOT NULL,
    roles text[] NOT NULL DEFAULT ARRAY['doctor'],
    active boolean NOT NULL DEFAULT true,
    password_hash text NOT NULL,
    last_login_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT staff_username_per_hospital UNIQUE (hospital_id, username)
);

ALTER TABLE staff ENABLE ROW LEVEL SECURITY;
ALTER TABLE staff FORCE ROW LEVEL SECURITY;
CREATE POLICY staff_tenant ON staff FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE TABLE patients (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    mrn text NOT NULL,
    full_name text NOT NULL,
    birth_date date NOT NULL,
    sex text NOT NULL CHECK (sex IN ('F', 'M', 'O')),
    blood_type text,
    weight_kg numeric(5,1),
    height_cm numeric(5,1),
    age_years smallint,
    phone text,
    email citext,
    address text,
    insurance_id text,
    emergency_contact text,
    deceased boolean NOT NULL DEFAULT false,
    search_vector tsvector GENERATED ALWAYS AS (
        to_tsvector('english', full_name || ' ' || coalesce(phone, '') || ' ' || coalesce(email::text, '') || ' ' || mrn)
    ) STORED,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT patients_mrn UNIQUE (hospital_id, mrn),
    CONSTRAINT patient_birth_check CHECK (birth_date > '1900-01-01' AND birth_date <= current_date),
    CONSTRAINT patient_weight_check CHECK (weight_kg IS NULL OR weight_kg BETWEEN 0.5 AND 500),
    CONSTRAINT patient_height_check CHECK (height_cm IS NULL OR height_cm BETWEEN 20 AND 250)
);

ALTER TABLE patients ENABLE ROW LEVEL SECURITY;
ALTER TABLE patients FORCE ROW LEVEL SECURITY;
CREATE POLICY patients_tenant ON patients FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE INDEX patients_search_gin ON patients USING gin (search_vector);
CREATE INDEX patients_name_trgm ON patients USING gin (full_name gin_trgm_ops);
CREATE INDEX patients_email_trgm ON patients USING gin (email gin_trgm_ops);
CREATE INDEX patients_hospital_created ON patients (hospital_id, created_at DESC);
CREATE INDEX patients_partial_deceased ON patients (id) WHERE deceased;

CREATE OR REPLACE FUNCTION app.set_patient_age() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    NEW.age_years := date_part('year', age(NEW.birth_date))::smallint;
    NEW.updated_at := now();
    RETURN NEW;
END;
$$;

CREATE TRIGGER patients_age
    BEFORE INSERT OR UPDATE ON patients
    FOR EACH ROW EXECUTE FUNCTION app.set_patient_age();

CREATE TABLE patient_allergies (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    patient_id uuid NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    allergen text NOT NULL,
    severity text NOT NULL CHECK (severity IN ('mild', 'moderate', 'severe', 'anaphylactic')),
    reaction text,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    recorded_by text,
    CONSTRAINT allergy_unique UNIQUE (hospital_id, patient_id, allergen)
);

ALTER TABLE patient_allergies ENABLE ROW LEVEL SECURITY;
ALTER TABLE patient_allergies FORCE ROW LEVEL SECURITY;
CREATE POLICY allergies_tenant ON patient_allergies FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE TABLE icd_codes (
    code text PRIMARY KEY,
    description text NOT NULL,
    category text NOT NULL
);

CREATE TABLE diagnoses (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    patient_id uuid NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    icd_code text NOT NULL REFERENCES icd_codes(code),
    provisional boolean NOT NULL DEFAULT true,
    note text,
    created_at timestamptz NOT NULL DEFAULT now(),
    created_by text
);

ALTER TABLE diagnoses ENABLE ROW LEVEL SECURITY;
ALTER TABLE diagnoses FORCE ROW LEVEL SECURITY;
CREATE POLICY diagnoses_tenant ON diagnoses FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE INDEX diagnoses_patient_idx ON diagnoses (patient_id, created_at DESC);
CREATE INDEX diagnoses_icd_idx ON diagnoses (icd_code);

CREATE TABLE visits (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    patient_id uuid NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('consultation', 'emergency', 'surgery', 'follow-up', 'phone')),
    reason text NOT NULL,
    entered_at timestamptz NOT NULL DEFAULT now(),
    closed_at timestamptz,
    entered_by text,
    closed_by text
);

ALTER TABLE visits ENABLE ROW LEVEL SECURITY;
ALTER TABLE visits FORCE ROW LEVEL SECURITY;
CREATE POLICY visits_tenant ON visits FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE INDEX visits_patient_idx ON visits (patient_id, entered_at DESC);
CREATE INDEX visits_open_idx ON visits (hospital_id, entered_at) WHERE closed_at IS NULL;

CREATE TABLE wards (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    code text NOT NULL,
    name text NOT NULL,
    UNIQUE (hospital_id, code)
);

ALTER TABLE wards ENABLE ROW LEVEL SECURITY;
ALTER TABLE wards FORCE ROW LEVEL SECURITY;
CREATE POLICY wards_tenant ON wards FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE TABLE beds (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    ward_id uuid NOT NULL REFERENCES wards(id),
    code text NOT NULL,
    UNIQUE (hospital_id, code)
);

ALTER TABLE beds ENABLE ROW LEVEL SECURITY;
ALTER TABLE beds FORCE ROW LEVEL SECURITY;
CREATE POLICY beds_tenant ON beds FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE TABLE admissions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    patient_id uuid NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    ward text NOT NULL,
    bed_id uuid REFERENCES beds(id),
    admitted_at timestamptz NOT NULL DEFAULT now(),
    discharged_at timestamptz,
    reason text,
    admitted_by text,
    discharged_by text,
    CONSTRAINT discharge_after_admission CHECK (discharged_at IS NULL OR discharged_at > admitted_at),
    CONSTRAINT bed_no_double_booking EXCLUDE USING gist (
        bed_id WITH =,
        tstzrange(admitted_at, COALESCE(discharged_at, 'infinity'::timestamptz)) WITH &&
    )
);

ALTER TABLE admissions ENABLE ROW LEVEL SECURITY;
ALTER TABLE admissions FORCE ROW LEVEL SECURITY;
CREATE POLICY admissions_tenant ON admissions FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE INDEX admissions_patient_idx ON admissions (patient_id, admitted_at DESC);
CREATE INDEX admissions_open_idx ON admissions (id) WHERE discharged_at IS NULL;

CREATE TABLE vital_signs (
    id uuid NOT NULL DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    patient_id uuid NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    heart_rate smallint,
    systolic_bp smallint,
    diastolic_bp smallint,
    temperature_c numeric(5,1),
    respiratory_rate smallint,
    spo2 smallint,
    weight_kg numeric(5,1),
    height_cm numeric(5,1),
    bmi numeric(4,1) GENERATED ALWAYS AS (
        round((NULLIF(weight_kg, 0) / (NULLIF(height_cm, 0) / 100.0) ^ 2)::numeric, 1)
    ) STORED,
    recorded_by text,
    CONSTRAINT vital_hr CHECK (heart_rate IS NULL OR heart_rate BETWEEN 30 AND 220),
    CONSTRAINT vital_sys CHECK (systolic_bp IS NULL OR systolic_bp BETWEEN 40 AND 300),
    CONSTRAINT vital_dia CHECK (diastolic_bp IS NULL OR diastolic_bp BETWEEN 20 AND 180),
    CONSTRAINT vital_temp CHECK (temperature_c IS NULL OR temperature_c BETWEEN 34 AND 43),
    CONSTRAINT vital_spo2 CHECK (spo2 IS NULL OR spo2 BETWEEN 40 AND 100),
    PRIMARY KEY (id, recorded_at)
) PARTITION BY RANGE (recorded_at);

ALTER TABLE vital_signs ENABLE ROW LEVEL SECURITY;
ALTER TABLE vital_signs FORCE ROW LEVEL SECURITY;
CREATE POLICY vital_signs_tenant ON vital_signs FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE INDEX vital_signs_patient_idx ON vital_signs (patient_id, recorded_at DESC);

CREATE OR REPLACE FUNCTION app.create_partition(p_table text, p_month date)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    v_part text;
    v_from timestamptz;
    v_to timestamptz;
BEGIN
    v_part := p_table || '_' || to_char(p_month, 'YYYYMM');
    IF NOT EXISTS (SELECT FROM pg_class WHERE relname = v_part) THEN
        v_from := p_month::timestamptz;
        v_to := (p_month + interval '1 month')::timestamptz;
        EXECUTE format(
            'CREATE TABLE %I PARTITION OF %I FOR VALUES FROM (%L) TO (%L)',
            v_part, p_table, v_from::text, v_to::text
        );
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION app.maintain_partitions()
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    v_month date;
    v_offset integer;
BEGIN
    -- previous month included so backfilled/near-boundary rows always land in
    -- an existing partition.
    FOR v_offset IN -1..5 LOOP
        v_month := (date_trunc('month', now()) + make_interval(months => v_offset))::date;
        PERFORM app.create_partition('vital_signs', v_month);
        PERFORM app.create_partition('lab_results', v_month);
    END LOOP;
END;
$$;

DO $sched$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_cron')
       AND NOT pg_is_in_recovery() THEN
        PERFORM cron.schedule('partition-monthly', '0 1 * * *',
            $c$SELECT app.maintain_partitions()$c$);
    END IF;
END
$sched$;

CREATE TABLE medications (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    generic_name text NOT NULL UNIQUE,
    brand_name text,
    route text NOT NULL,
    strength text NOT NULL,
    controlled_substance boolean NOT NULL DEFAULT false
);

CREATE TABLE prescriptions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    patient_id uuid NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    medication_id uuid NOT NULL REFERENCES medications(id),
    drug_name text NOT NULL,
    dosage numeric(8,2) NOT NULL CHECK (dosage > 0),
    unit text NOT NULL,
    frequency jsonb NOT NULL,
    route text NOT NULL,
    duration_days smallint NOT NULL CHECK (duration_days BETWEEN 1 AND 365),
    prescribed_by text NOT NULL,
    issued_at timestamptz NOT NULL DEFAULT now(),
    refills smallint NOT NULL DEFAULT 0
);

ALTER TABLE prescriptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE prescriptions FORCE ROW LEVEL SECURITY;
CREATE POLICY prescriptions_tenant ON prescriptions FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE INDEX prescriptions_patient_idx ON prescriptions (patient_id, issued_at DESC);

CREATE TABLE lab_panels (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    code text NOT NULL UNIQUE,
    name text NOT NULL
);

CREATE TABLE lab_orders (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    patient_id uuid NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    panel_id uuid NOT NULL REFERENCES lab_panels(id),
    panel text NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'performed', 'result_done')),
    priority text NOT NULL CHECK (priority IN ('routine', 'urgent', 'stat')),
    requested_by text NOT NULL,
    requested_at timestamptz NOT NULL DEFAULT now(),
    performed_at timestamptz,
    completed_at timestamptz
);

ALTER TABLE lab_orders ENABLE ROW LEVEL SECURITY;
ALTER TABLE lab_orders FORCE ROW LEVEL SECURITY;
CREATE POLICY lab_orders_tenant ON lab_orders FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE INDEX lab_orders_patient_idx ON lab_orders (patient_id, requested_at DESC);

CREATE TABLE lab_reference_ranges (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    panel_id uuid NOT NULL REFERENCES lab_panels(id),
    analysis text NOT NULL,
    unit text NOT NULL,
    low numeric(10,1) NOT NULL,
    high numeric(10,1) NOT NULL,
    UNIQUE (panel_id, analysis)
);

CREATE TABLE lab_results (
    id uuid NOT NULL DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    order_id uuid NOT NULL REFERENCES lab_orders(id) ON DELETE CASCADE,
    analysis text NOT NULL,
    value numeric(12,2) NOT NULL,
    unit text NOT NULL,
    ref_low numeric(10,1),
    ref_high numeric(10,1),
    flag text NOT NULL DEFAULT 'N' CHECK (flag IN ('N', 'L', 'H')),
    result_date timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (id, result_date)
) PARTITION BY RANGE (result_date);

ALTER TABLE lab_results ENABLE ROW LEVEL SECURITY;
ALTER TABLE lab_results FORCE ROW LEVEL SECURITY;
CREATE POLICY lab_results_tenant ON lab_results FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE INDEX lab_results_order_idx ON lab_results (order_id, result_date DESC);

CREATE TABLE pharmacy_stock (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    medication_id uuid NOT NULL REFERENCES medications(id),
    batch text NOT NULL,
    expiry_date date NOT NULL,
    quantity integer NOT NULL CHECK (quantity >= 0),
    unit_cost_cents bigint NOT NULL CHECK (unit_cost_cents >= 0),
    UNIQUE (hospital_id, medication_id, batch)
);

ALTER TABLE pharmacy_stock ENABLE ROW LEVEL SECURITY;
ALTER TABLE pharmacy_stock FORCE ROW LEVEL SECURITY;
CREATE POLICY pharmacy_tenant ON pharmacy_stock FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE TABLE invoices (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    patient_id uuid NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    visit_id uuid REFERENCES visits(id),
    reference text NOT NULL UNIQUE,
    amount_cents bigint NOT NULL DEFAULT 0 CHECK (amount_cents >= 0),
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'paid', 'refunded', 'voided')),
    created_at timestamptz NOT NULL DEFAULT now(),
    paid_at timestamptz
);

ALTER TABLE invoices ENABLE ROW LEVEL SECURITY;
ALTER TABLE invoices FORCE ROW LEVEL SECURITY;
CREATE POLICY invoices_tenant ON invoices FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE INDEX invoices_patient_idx ON invoices (patient_id, created_at DESC);
CREATE INDEX invoices_status_idx ON invoices (hospital_id, status);

CREATE TABLE invoice_lines (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    invoice_id uuid NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    description text NOT NULL,
    unit_price_cents bigint NOT NULL CHECK (unit_price_cents > 0),
    amount integer NOT NULL CHECK (amount > 0)
);

ALTER TABLE invoice_lines ENABLE ROW LEVEL SECURITY;
ALTER TABLE invoice_lines FORCE ROW LEVEL SECURITY;
CREATE POLICY invoice_lines_tenant ON invoice_lines FOR ALL TO anamnesis_app
    USING (EXISTS (
        SELECT 1 FROM invoices i
        WHERE i.id = invoice_lines.invoice_id
          AND i.hospital_id = app.current_hospital_id()
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM invoices i
        WHERE i.id = invoice_lines.invoice_id
          AND i.hospital_id = app.current_hospital_id()
    ));

CREATE TABLE payments (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    invoice_id uuid NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    amount_cents bigint NOT NULL CHECK (amount_cents > 0),
    method text NOT NULL CHECK (method IN ('card', 'cash', 'transfer', 'insurance')),
    received_at timestamptz NOT NULL DEFAULT now(),
    received_by text NOT NULL
);

ALTER TABLE payments ENABLE ROW LEVEL SECURITY;
ALTER TABLE payments FORCE ROW LEVEL SECURITY;
CREATE POLICY payments_tenant ON payments FOR ALL TO anamnesis_app
    USING (hospital_id = app.current_hospital_id())
    WITH CHECK (hospital_id = app.current_hospital_id());

CREATE TABLE outbox (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    patient_id uuid REFERENCES patients(id),
    event_type text NOT NULL,
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    dispatched_at timestamptz,
    attempts integer NOT NULL DEFAULT 0
);

CREATE INDEX outbox_pending_idx ON outbox (created_at) WHERE dispatched_at IS NULL;

CREATE TABLE idempotency_keys (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    key text NOT NULL,
    endpoint text NOT NULL,
    response_json jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (hospital_id, key)
);

CREATE SEQUENCE app.mrn_seq START 100000;

CREATE OR REPLACE FUNCTION app.next_mrn() RETURNS text
LANGUAGE sql VOLATILE AS $$
    SELECT 'MRN-' || (nextval('app.mrn_seq'))::text
$$;

CREATE OR REPLACE FUNCTION app.bill_visit(p_visit uuid, p_clerk text)
RETURNS invoices
LANGUAGE plpgsql
AS $$
DECLARE
    v_patient uuid;
    v_hospital uuid;
    v_kind text;
    v_invoice invoices%ROWTYPE;
    v_cents bigint;
BEGIN
    SELECT patient_id, hospital_id, kind INTO v_patient, v_hospital, v_kind
    FROM visits
    WHERE id = p_visit AND closed_at IS NOT NULL;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'visit not closed or not found: ' || p_visit::text;
    END IF;

    v_cents := CASE v_kind
        WHEN 'emergency' THEN 15000
        WHEN 'surgery' THEN 8000
        ELSE 4000
    END;

    INSERT INTO invoices (hospital_id, patient_id, visit_id, reference, amount_cents)
    VALUES (v_hospital, v_patient, p_visit, 'INV-' || upper(substr(gen_random_uuid()::text, 1, 8)), v_cents)
    RETURNING * INTO v_invoice;

    INSERT INTO invoice_lines (invoice_id, description, unit_price_cents, amount)
    VALUES (v_invoice.id, v_kind || ' visit fee', v_cents, 1);

    RETURN v_invoice;
END;
$$;

CREATE OR REPLACE FUNCTION app.charge_invoice(p_invoice uuid, p_clerk text, p_amount_cents bigint)
RETURNS boolean
LANGUAGE plpgsql
AS $$
DECLARE
    v_hospital uuid;
    v_patient uuid;
    v_status text;
BEGIN
    SELECT hospital_id, patient_id, status INTO v_hospital, v_patient, v_status
    FROM invoices
    WHERE id = p_invoice
    FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'invoice not found: ' || p_invoice::text;
    END IF;

    IF v_status IS DISTINCT FROM 'pending' THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'invoice is ' || v_status || ', only pending can be charged';
    END IF;

    IF p_amount_cents <= 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'payment amount must be positive';
    END IF;

    UPDATE invoices SET status = 'paid', paid_at = now() WHERE id = p_invoice;

    INSERT INTO payments (hospital_id, invoice_id, amount_cents, method, received_by)
    VALUES (v_hospital, p_invoice, p_amount_cents, 'card', p_clerk);

    INSERT INTO outbox (hospital_id, patient_id, event_type, payload)
    VALUES (
        v_hospital,
        v_patient,
        'invoice.paid',
        jsonb_build_object('invoice_id', p_invoice, 'amount_cents', p_amount_cents)
    );

    RETURN true;
END;
$$;

GRANT USAGE ON SEQUENCE app.mrn_seq TO anamnesis_app;
GRANT EXECUTE ON FUNCTION app.next_mrn() TO anamnesis_app;
GRANT EXECUTE ON FUNCTION app.bill_visit(uuid, text) TO anamnesis_app;
GRANT EXECUTE ON FUNCTION app.charge_invoice(uuid, text, bigint) TO anamnesis_app;
GRANT EXECUTE ON FUNCTION app.maintain_partitions() TO anamnesis_app;

CREATE TABLE app_audit (
    id bigserial PRIMARY KEY,
    hospital_id uuid NOT NULL REFERENCES hospitals(id),
    table_name text NOT NULL,
    record_key jsonb NOT NULL,
    operation text NOT NULL CHECK (operation IN ('INSERT', 'UPDATE', 'DELETE')),
    old_json jsonb,
    new_json jsonb,
    changed_by text,
    changed_at timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE app_audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE app_audit FORCE ROW LEVEL SECURITY;
CREATE POLICY audit_tenant ON app_audit FOR SELECT TO anamnesis_app
    USING (hospital_id = app.current_hospital_id());

-- SECURITY DEFINER so the audit insert runs as the (privileged) owner: the
-- least-privileged app role can trigger audit writes but cannot INSERT/UPDATE/
-- DELETE app_audit directly, keeping the trail tamper-evident.
CREATE OR REPLACE FUNCTION app.audit_row() RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, app
AS $$
DECLARE
    v_hospital uuid;
    v_key jsonb;
    v_old jsonb;
    v_new jsonb;
BEGIN
    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        v_hospital := NEW.hospital_id;
    ELSE
        v_hospital := OLD.hospital_id;
    END IF;

    v_key := jsonb_build_object(
        'id', CASE WHEN TG_OP = 'DELETE' THEN OLD.id ELSE NEW.id END
    );
    v_old := CASE WHEN TG_OP IN ('UPDATE', 'DELETE') THEN to_jsonb(OLD) END;
    v_new := CASE WHEN TG_OP IN ('INSERT', 'UPDATE') THEN to_jsonb(NEW) END;

    INSERT INTO app_audit (hospital_id, table_name, record_key, operation, old_json, new_json, changed_by)
    VALUES (v_hospital, TG_TABLE_NAME, v_key, TG_OP, v_old, v_new, app.current_staff_id());

    RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE TRIGGER patients_audit AFTER INSERT OR UPDATE OR DELETE ON patients FOR EACH ROW EXECUTE FUNCTION app.audit_row();
CREATE TRIGGER prescriptions_audit AFTER INSERT OR UPDATE OR DELETE ON prescriptions FOR EACH ROW EXECUTE FUNCTION app.audit_row();
CREATE TRIGGER admissions_audit AFTER INSERT OR UPDATE OR DELETE ON admissions FOR EACH ROW EXECUTE FUNCTION app.audit_row();
CREATE TRIGGER invoices_audit AFTER INSERT OR UPDATE OR DELETE ON invoices FOR EACH ROW EXECUTE FUNCTION app.audit_row();
CREATE TRIGGER visits_audit AFTER INSERT OR UPDATE OR DELETE ON visits FOR EACH ROW EXECUTE FUNCTION app.audit_row();

CREATE MATERIALIZED VIEW app.daily_census AS
SELECT
    hospital_id,
    count(*)                       AS total_admissions,
    count(*) FILTER (WHERE discharged_at IS NULL) AS inpatients
FROM admissions
GROUP BY hospital_id;

CREATE UNIQUE INDEX daily_census_hospital ON app.daily_census (hospital_id);

CREATE VIEW app.open_admissions AS
SELECT a.id, a.patient_id, p.full_name, b.code AS bed, a.admitted_at
FROM admissions a
JOIN patients p ON p.id = a.patient_id
LEFT JOIN beds b ON b.id = a.bed_id
WHERE a.discharged_at IS NULL;

DO $sched$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_cron')
       AND NOT pg_is_in_recovery() THEN
        PERFORM cron.schedule('census-refresh', '*/15 * * * *',
            $c$REFRESH MATERIALIZED VIEW CONCURRENTLY app.daily_census$c$);
    END IF;
END
$sched$;


