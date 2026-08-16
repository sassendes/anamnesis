#!/usr/bin/env bash
set -euo pipefail
COUNT="${1:-1000}"
echo "seeding $COUNT patients into anamnesis..."
kubectl exec -i anamnesis-1 -- psql -U postgres -d anamnesis <<EOF
DO \$\$
DECLARE
    h1 uuid := '11111111-1111-1111-1111-111111111111';
    h2 uuid := '22222222-2222-2222-2222-222222222222';
    first_names text[] := ARRAY['Emma','Lucas','Chloe','Nathan','Lea','Hugo','Manon','Louis','Sarah','Adam','Ines','Gabriel','Jade','Raphael','Alice','Arthur','Lina','Jules','Camille','Ethan','Zoe','Noah','Eva','Tom','Anna','Sacha','Rose','Liam','Mila','Yanis','Nour','Rayan','Lola','Aaron','Julia','Mehdi','Clara','Ali','Maya','Amir'];
    last_names text[] := ARRAY['Martin','Bernard','Dubois','Thomas','Robert','Petit','Durand','Leroy','Moreau','Simon','Laurent','Michel','Garcia','David','Bertrand','Roux','Vincent','Fournier','Morel','Girard','Andre','Lefevre','Mercier','Blanc','Guerin','Boyer','Garnier','Chevalier','Francois','Legrand','Ben Ali','Trabelsi','Gharbi','Jelassi','Haddad','Khelifi','Bouazizi','Chaabane','Nasri','Ayari'];
    bloods text[] := ARRAY['O+','A+','B+','AB+','O-','A-','B-','AB-'];
    sexes text[] := ARRAY['F','M','O'];
    i int; fn text; ln text; hosp uuid; hpref text;
    half int := ${COUNT} / 2;
BEGIN
    FOR i IN 1..${COUNT} LOOP
        IF i <= half THEN hosp := h1; hpref := 'HCP'; ELSE hosp := h2; hpref := 'CPV'; END IF;
        fn := first_names[1 + floor(random()*array_length(first_names,1))::int];
        ln := last_names[1 + floor(random()*array_length(last_names,1))::int];
        INSERT INTO patients (hospital_id, mrn, full_name, birth_date, sex, blood_type, weight_kg, height_cm, phone, email, address, insurance_id)
        VALUES (
            hosp,
            hpref || '-P' || lpad(i::text, 6, '0'),
            fn || ' ' || ln,
            (CURRENT_DATE - ((3650 + floor(random()*29200))::int))::date,
            sexes[1 + floor(random()*3)::int],
            bloods[1 + floor(random()*8)::int],
            round((45 + random()*55)::numeric, 1),
            round((150 + random()*45)::numeric, 1),
            '+216' || (20000000 + floor(random()*79999999))::bigint::text,
            lower(fn || '.' || ln || i::text || '@example.tn'),
            (1 + floor(random()*200))::int::text || ' Rue de la Sante, Tunis',
            'INS-' || upper(substr(md5(random()::text), 1, 10))
        );
    END LOOP;
END \$\$;
SELECT hospital_id, count(*) FROM patients GROUP BY hospital_id;
EOF
echo "done."
