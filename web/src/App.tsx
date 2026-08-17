import { FormEvent, useCallback, useEffect, useState } from "react";
import { api, getToken, setToken, ApiError } from "./api";

type Page = "dashboard" | "wards" | "patients" | "audit";

const HOSPITALS = [
  { id: "11111111-1111-1111-1111-111111111111", name: "Hopital Central de Plaisir" },
  { id: "22222222-2222-2222-2222-222222222222", name: "Clinique du Parc Versailles" },
];

interface Row {
  id?: string;
  [key: string]: unknown;
}

interface Patient extends Row {
  id: string;
  full_name: string;
  sex: string;
  birth_date: string;
  blood_type: string | null;
  weight_kg: number | null;
  height_cm: number | null;
  phone: string | null;
  email: string | null;
  address: string | null;
  insurance_id: string | null;
  emergency_contact: string | null;
}

function DataTable({
  rows,
  onEdit,
}: {
  rows: Row[];
  onEdit?: (row: Row) => void;
}) {
  if (!rows.length) return <p className="muted">nothing here yet</p>;
  const cols = Object.keys(rows[0]);
  return (
    <table>
      <thead>
        <tr>
          {onEdit && <th></th>}
          {cols.map((c) => (
            <th key={c}>{c}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr key={i}>
            {onEdit && (
              <td>
                <button className="mini" onClick={() => onEdit(r)}>
                  edit
                </button>
              </td>
            )}
            {cols.map((c) => (
              <td key={c}>{JSON.stringify(r[c])}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Field({
  label,
  value,
  onChange,
  type = "text",
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  type?: string;
  placeholder?: string;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      <input
        type={type}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}

type PatientFormState = {
  __id?: string;
  full_name: string;
  birth_date: string;
  sex: string;
  blood_type: string;
  weight_kg: string;
  height_cm: string;
  phone: string;
  email: string;
  address: string;
  insurance_id: string;
  emergency_contact: string;
};

const EMPTY_FORM: PatientFormState = {
  full_name: "",
  birth_date: "",
  sex: "F",
  blood_type: "",
  weight_kg: "",
  height_cm: "",
  phone: "",
  email: "",
  address: "",
  insurance_id: "",
  emergency_contact: "",
};

function s(v: string): string | null {
  const t = v.trim();
  return t === "" ? null : t;
}
function num(v: string): number | null {
  const t = v.trim();
  if (t === "") return null;
  const parsed = Number(t);
  return Number.isFinite(parsed) ? parsed : null;
}

function PatientForm({
  mode,
  initial,
  onCancel,
  onSaved,
}: {
  mode: "create" | "edit";
  initial: PatientFormState;
  onCancel: () => void;
  onSaved: () => void;
}) {
  const [form, setForm] = useState<PatientFormState>(initial);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  function set<K extends keyof PatientFormState>(key: K, val: string) {
    setForm((f) => ({ ...f, [key]: val }));
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    setError("");
    setBusy(true);
    try {
      if (mode === "create") {
        const body = {
          full_name: form.full_name.trim(),
          birth_date: form.birth_date,
          sex: form.sex,
          blood_type: s(form.blood_type),
          weight_kg: num(form.weight_kg),
          height_cm: num(form.height_cm),
          phone: s(form.phone),
          email: s(form.email),
          address: s(form.address),
          insurance_id: s(form.insurance_id),
          emergency_contact: s(form.emergency_contact),
        };
        await api("/patients", { method: "POST", body: JSON.stringify(body) });
      } else {
        const body = {
          full_name: s(form.full_name),
          blood_type: s(form.blood_type),
          weight_kg: num(form.weight_kg),
          height_cm: num(form.height_cm),
          phone: s(form.phone),
          email: s(form.email),
          address: s(form.address),
          insurance_id: s(form.insurance_id),
          emergency_contact: s(form.emergency_contact),
        };
        await api("/patients/" + form.__id, {
          method: "PATCH",
          body: JSON.stringify(body),
        });
      }
      onSaved();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="pform" onSubmit={submit}>
      <h3>{mode === "create" ? "new patient" : "edit patient"}</h3>
      <Field label="full name" value={form.full_name} onChange={(v) => set("full_name", v)} />
      {mode === "create" && (
        <>
          <Field
            label="birth date"
            type="date"
            value={form.birth_date}
            onChange={(v) => set("birth_date", v)}
          />
          <label className="field">
            <span>sex</span>
            <select value={form.sex} onChange={(e) => set("sex", e.target.value)}>
              <option value="F">F</option>
              <option value="M">M</option>
              <option value="O">O</option>
            </select>
          </label>
        </>
      )}
      <Field label="blood type" value={form.blood_type} onChange={(v) => set("blood_type", v)} placeholder="e.g. O+" />
      <Field label="weight (kg)" value={form.weight_kg} onChange={(v) => set("weight_kg", v)} />
      <Field label="height (cm)" value={form.height_cm} onChange={(v) => set("height_cm", v)} />
      <Field label="phone" value={form.phone} onChange={(v) => set("phone", v)} />
      <Field label="email" value={form.email} onChange={(v) => set("email", v)} />
      <Field label="address" value={form.address} onChange={(v) => set("address", v)} />
      <Field label="insurance id" value={form.insurance_id} onChange={(v) => set("insurance_id", v)} />
      <Field label="emergency contact" value={form.emergency_contact} onChange={(v) => set("emergency_contact", v)} />
      {error && <p className="error">{error}</p>}
      <div className="row">
        <button type="submit" disabled={busy}>
          {busy ? "saving..." : "save"}
        </button>
        <button type="button" className="ghost" onClick={onCancel}>
          cancel
        </button>
      </div>
    </form>
  );
}

export function App() {
  const [page, setPage] = useState<Page>("dashboard");
  const [token, setTokenState] = useState<string | null>(() => getToken());
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [hospital, setHospital] = useState(HOSPITALS[0].id);
  const [error, setError] = useState("");
  const [dashboard, setDashboard] = useState<Record<string, unknown> | null>(null);
  const [rows, setRows] = useState<Row[]>([]);
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState<Patient | "create" | null>(null);

  const load = useCallback(async (p: Page) => {
    const map: Record<Page, { path: string; key: string }> = {
      dashboard: { path: "/dashboard/stats", key: "" },
      wards: { path: "/wards", key: "wards" },
      patients: { path: "/patients", key: "patients" },
      audit: { path: "/audit?page=50", key: "audit" },
    };
    try {
      const { path, key } = map[p];
      const data = await api<Record<string, unknown>>(path);
      if (p === "dashboard") {
        setDashboard(data);
      } else {
        setRows((data[key] as Row[]) ?? []);
      }
      setError("");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    if (token) load(page);
  }, [page, token, load]);

  async function login(e: FormEvent) {
    e.preventDefault();
    setError("");
    setBusy(true);
    try {
      const res = await api<{ token: string }>("/auth/login", {
        method: "POST",
        body: JSON.stringify({ username, password, hospital_id: hospital }),
      });
      setToken(res.token);
      setTokenState(res.token);
      setUsername("");
      setPassword("");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  function logout() {
    setToken(null);
    setTokenState(null);
  }

  function formFromPatient(p: Patient): PatientFormState {
    return {
      __id: p.id,
      full_name: p.full_name ?? "",
      birth_date: p.birth_date ?? "",
      sex: p.sex ?? "F",
      blood_type: p.blood_type ?? "",
      weight_kg: p.weight_kg != null ? String(p.weight_kg) : "",
      height_cm: p.height_cm != null ? String(p.height_cm) : "",
      phone: p.phone ?? "",
      email: p.email ?? "",
      address: p.address ?? "",
      insurance_id: p.insurance_id ?? "",
      emergency_contact: p.emergency_contact ?? "",
    };
  }

  if (!token) {
    return (
      <main className="login">
        <form className="box" onSubmit={login}>
          <h1>Anamnesis</h1>
          <label className="field">
            <span>hospital</span>
            <select value={hospital} onChange={(e) => setHospital(e.target.value)}>
              {HOSPITALS.map((h) => (
                <option key={h.id} value={h.id}>
                  {h.name}
                </option>
              ))}
            </select>
          </label>
          <input
            placeholder="username"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            autoComplete="username"
          />
          <input
            placeholder="password"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="current-password"
          />
          {error && <p className="error">{error}</p>}
          <button type="submit" disabled={busy}>
            {busy ? "signing in..." : "sign in"}
          </button>
        </form>
      </main>
    );
  }

  return (
    <main>
      <nav>
        <span className="brand">Anamnesis</span>
        {(["dashboard", "wards", "patients", "audit"] as Page[]).map((p) => (
          <button
            key={p}
            className={"nav" + (page === p ? " active" : "")}
            onClick={() => {
              setEditing(null);
              setPage(p);
            }}
          >
            {p}
          </button>
        ))}
        <button className="nav right" onClick={logout}>
          sign out
        </button>
      </nav>

      {page === "dashboard" ? (
        <section className="grid">
          <div className="card">
            <h2>API status</h2>
            <pre>{JSON.stringify(dashboard, null, 2)}</pre>
          </div>
          <div className="card">
            <h2>About</h2>
            <p className="muted">
              Tenant-isolated hospital data. The audit page shows row-level
              changes with old/new values.
            </p>
          </div>
        </section>
      ) : page === "patients" ? (
        <section className="card">
          <div className="cardhead">
            <h2>patients</h2>
            <button className="mini" onClick={() => setEditing("create")}>
              + add patient
            </button>
          </div>

          {editing === "create" && (
            <PatientForm
              mode="create"
              initial={EMPTY_FORM}
              onCancel={() => setEditing(null)}
              onSaved={() => {
                setEditing(null);
                load("patients");
              }}
            />
          )}
          {editing && editing !== "create" && (
            <PatientForm
              mode="edit"
              initial={formFromPatient(editing)}
              onCancel={() => setEditing(null)}
              onSaved={() => {
                setEditing(null);
                load("patients");
              }}
            />
          )}

          <DataTable rows={rows} onEdit={(r) => setEditing(r as Patient)} />
          {error && <p className="error">{error}</p>}
        </section>
      ) : (
        <section className="card">
          <h2>{page}</h2>
          <DataTable rows={rows} />
          {error && <p className="error">{error}</p>}
        </section>
      )}
    </main>
  );
}
