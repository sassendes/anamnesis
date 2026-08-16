import http from "k6/http";
import { check, sleep } from "k6";

export const options = {
  scenarios: {
    smoke: {
      executor: "ramping-vus",
      startVUs: 1,
      stages: [
        { duration: "30s", target: 10 },
        { duration: "1m", target: 40 },
        { duration: "30s", target: 10 },
        { duration: "10s", target: 0 },
      ],
    },
  },
  thresholds: {
    http_req_failed: ["rate<0.02"],
    http_req_duration: ["p(95)<800"],
  },
};

const BASE = __ENV.BASE_URL || "http://localhost:8080";

export default function () {
  const login = http.post(
    `${BASE}/api/v1/auth/login`,
    JSON.stringify({
      username: __ENV.LOGIN_USER || "sysadmin",
      password: __ENV.LOGIN_PASS || "doctor123",
      hospital_id: __ENV.HOSPITAL_ID || "11111111-1111-1111-1111-111111111111",
    }),
    { headers: { "content-type": "application/json" } }
  );
  check(login, { "login ok": (r) => r.status === 200 });
  const token = login.json("token");
  const auth = { authorization: `Bearer ${token}` };

  const wards = http.get(`${BASE}/api/v1/wards`, { headers: auth });
  check(wards, { "wards ok": (r) => r.status === 200 });

  const patients = http.get(`${BASE}/api/v1/patients?limit=20`, { headers: auth });
  check(patients, { "patients ok": (r) => r.status === 200 });

  sleep(1);
}