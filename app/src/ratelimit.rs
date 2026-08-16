use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Window {
    hits: u32,
    started: Instant,
}

struct Store {
    map: HashMap<String, Window>,
}

impl Store {
    fn allow(&mut self, key: &str, limit: u32, window: Duration) -> bool {
        let now = Instant::now();
        let entry = self.map.entry(key.to_string()).or_insert(Window {
            hits: 0,
            started: now,
        });
        if entry.started.elapsed() > window {
            entry.hits = 0;
            entry.started = now;
        }
        entry.hits += 1;
        entry.hits <= limit
    }

    fn prune(&mut self, window: Duration) {
        let now = Instant::now();
        self.map
            .retain(|_, w| now.duration_since(w.started) < window);
    }
}

static LOGIN_STORE: std::sync::OnceLock<Mutex<Store>> = std::sync::OnceLock::new();

fn login_store() -> &'static Mutex<Store> {
    LOGIN_STORE.get_or_init(|| {
        Mutex::new(Store {
            map: HashMap::new(),
        })
    })
}

pub fn login_allowed(client: &str) -> bool {
    let mut store = login_store().lock().expect("rate limit store poisoned");
    store.prune(Duration::from_secs(60));
    store.allow(client, 20, Duration::from_secs(60))
}
