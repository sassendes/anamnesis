import { FormEvent, useCallback, useEffect, useState } from "react";
import { api, getToken, setToken, ApiError } from "./api";

type Page = "dashboard" | "wards" | "patients" | "audit";

interface Row {
  id?: string;
  [key: string]: unknown;
}

function DataTable({ rows }: { rows: Row[] }) {
  if (!rows.length) return <p className="muted">nothing here yet</p>;
  const cols = Object.keys(rows[0]);
  return (
    <table>
      <thead>
        <tr>
          {cols.map((c) => (
            <th key={c}>{c}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr key={i}>
            {cols.map((c) => (
              <td key={c}>{JSON.stringify(r[c])}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function App() {
  const [page, setPage] = useState<Page>("dashboard");
  const [token, setTokenState] = useState<string | null>(() => getToken());
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [hospital, setHospital] = useState("");
  const [error, setError] = useState("");
  const [dashboard, setDashboard] = useState<Record<string, unknown> | null>(null);
  const [rows, setRows] = useState<Row[]>([]);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async (p: Page) => {
    // path + the response key each endpoint wraps its rows in.
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
        body: JSON.stringify({ username, password, hospital_id: hospital.trim() }),
      });
      setToken(res.token);
      setTokenState(res.token);
      setUsername("");
      setPassword("");
      setHospital("");
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

  if (!token) {
    return (
      <main className="login">
        <form className="box" onSubmit={login}>
          <h1>Anamnesis</h1>
          <input
            placeholder="hospital id (uuid)"
            value={hospital}
            onChange={(e) => setHospital(e.target.value)}
            autoComplete="off"
          />
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
            {busy ? "signing in…" : "sign in"}
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
            onClick={() => setPage(p)}
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
              changes with old/new values. Load meters run against this UI.
            </p>
          </div>
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