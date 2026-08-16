-- idempotent invoicing: one active invoice per visit, enforced at the DB
CREATE UNIQUE INDEX IF NOT EXISTS uq_one_invoice_per_visit
    ON invoices (visit_id)
    WHERE visit_id IS NOT NULL AND status <> 'voided';

-- a bed can hold at most one open admission
CREATE UNIQUE INDEX IF NOT EXISTS uq_one_open_admission_per_bed
    ON admissions (bed_id)
    WHERE bed_id IS NOT NULL AND discharged_at IS NULL;

-- bill_visit becomes idempotent: a second call returns the existing invoice
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
    SELECT * INTO v_invoice FROM invoices
    WHERE visit_id = p_visit AND status <> 'voided';

    IF NOT FOUND THEN
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
    END IF;

    RETURN v_invoice;
END;
$$;