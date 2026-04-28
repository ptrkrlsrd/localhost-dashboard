#[macro_use]
extern crate rocket;

use handlebars::Handlebars;
use listeners::get_all;
use rocket::response::content::Html;
use rocket::tokio::sync::{Mutex, RwLock};
use rocket::State;
use scraper::{Html as ScraperHtml, Selector};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};

#[derive(Deserialize, Clone)]
struct DashboardConfig {
    ignored_processes: Vec<String>,
    dev_port_min: u16,
    dev_port_max: u16,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct ProcessInfo {
    name: String,
    port: u16,
    addr: String,
    memory_mb: u64,
    pid: u32,
    title: Option<String>,
}

#[derive(Clone, Serialize, Default)]
struct DashboardData {
    local_apps: Vec<ProcessInfo>,
    services: Vec<ProcessInfo>,
}

struct RefreshContext {
    last_refresh: Option<Instant>,
    title_by_addr: HashMap<String, String>,
    title_attempted: HashSet<String>,
}

impl Default for RefreshContext {
    fn default() -> Self {
        Self {
            last_refresh: None,
            title_by_addr: HashMap::new(),
            title_attempted: HashSet::new(),
        }
    }
}

#[derive(Clone)]
struct AppState {
    data: Arc<RwLock<DashboardData>>,
    refresh_ctx: Arc<Mutex<RefreshContext>>,
    client: reqwest::Client,
    config: DashboardConfig,
}

const REFRESH_MIN_INTERVAL: Duration = Duration::from_secs(10);

async fn refresh_if_due(state: &AppState) {
    let mut guard = state.refresh_ctx.lock().await;
    let now = Instant::now();
    if let Some(t) = guard.last_refresh {
        if now.duration_since(t) < REFRESH_MIN_INTERVAL {
            return;
        }
    }
    guard.last_refresh = Some(now);
    let new_data = {
        let RefreshContext {
            ref mut title_by_addr,
            ref mut title_attempted,
            ..
        } = &mut *guard;
        refresh_dashboard_data(
            &state.client,
            &state.config,
            title_by_addr,
            title_attempted,
        )
        .await
    };
    drop(guard);
    let mut data = state.data.write().await;
    *data = new_data;
}

const KNOWN_SERVICES: &[&str] = &["postgres", "redis-server", "docker", "mysqld", "node"];

fn is_dev_port(port: u16, config: &DashboardConfig) -> bool {
    port >= config.dev_port_min && port <= config.dev_port_max
}

fn is_service(name: &str, port: u16) -> bool {
    let name_lower = name.to_lowercase();
    KNOWN_SERVICES.iter().any(|&s| name_lower.contains(s))
        || matches!(port, 5432 | 6379 | 3306 | 27017)
}

async fn fetch_title(client: &reqwest::Client, addr: &str) -> Option<String> {
    let url = format!("http://{}", addr);
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let body = resp.text().await.ok()?;
    let document = ScraperHtml::parse_document(&body);
    let selector = Selector::parse("title").ok()?;

    document
        .select(&selector)
        .next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .filter(|t| !t.is_empty())
}

async fn refresh_dashboard_data(
    client: &reqwest::Client,
    config: &DashboardConfig,
    title_by_addr: &mut HashMap<String, String>,
    title_attempted: &mut HashSet<String>,
) -> DashboardData {
    let config_clone = config.clone();

    let processes_to_fetch = rocket::tokio::task::spawn_blocking(move || {
        let mut sys = System::new();
        sys.refresh_processes();

        let mut results = Vec::new();
        let mut seen_ports = HashSet::new();

        if let Ok(listeners) = get_all() {
            for listener in listeners {
                let port = listener.socket.port();
                if !seen_ports.insert(port) {
                    continue;
                }

                let name = listener.process.name.clone();
                let name_lower = name.to_lowercase();

                if config_clone
                    .ignored_processes
                    .iter()
                    .any(|s| name_lower.contains(&s.to_lowercase()))
                {
                    continue;
                }

                let ip = listener.socket.ip();
                let addr = if ip.is_unspecified() {
                    format!("127.0.0.1:{}", port)
                } else {
                    listener.socket.to_string()
                };

                let pid = listener.process.pid;
                let memory_mb = sys
                    .process(Pid::from(pid as usize))
                    .map(|p| p.memory() / 1024 / 1024)
                    .unwrap_or(0);

                results.push(ProcessInfo {
                    name,
                    port,
                    addr,
                    memory_mb,
                    pid,
                    title: None,
                });
            }
        }
        results
    })
    .await
    .unwrap_or_default();

    let current_addrs: HashSet<String> = processes_to_fetch
        .iter()
        .map(|p| p.addr.clone())
        .collect();
    title_by_addr.retain(|a, _| current_addrs.contains(a));
    title_attempted.retain(|a| current_addrs.contains(a));

    let mut title_futures = Vec::new();
    for info in &processes_to_fetch {
        let is_web = is_dev_port(info.port, config)
            || matches!(info.port, 80 | 443 | 8080 | 8888 | 5000 | 7000 | 4000);
        if !is_web {
            continue;
        }
        let addr = &info.addr;
        if title_by_addr.contains_key(addr) || title_attempted.contains(addr) {
            continue;
        }
        let addr = addr.clone();
        let client_clone = client.clone();
        title_futures.push(async move {
            let title = fetch_title(&client_clone, &addr).await;
            (addr, title)
        });
    }

    let titles = futures::future::join_all(title_futures).await;
    for (addr, title) in titles {
        title_attempted.insert(addr.clone());
        if let Some(t) = title {
            title_by_addr.insert(addr, t);
        }
    }

    let mut local_apps = Vec::new();
    let mut services = Vec::new();

    for mut info in processes_to_fetch {
        info.title = title_by_addr.get(&info.addr).cloned();

        if is_dev_port(info.port, config) && !is_service(&info.name, info.port) {
            local_apps.push(info);
        } else if is_service(&info.name, info.port) || info.port < config.dev_port_min {
            services.push(info);
        }
    }

    local_apps.sort_by_key(|p| p.port);
    services.sort_by_key(|p| p.port);

    DashboardData {
        local_apps,
        services,
    }
}

#[get("/")]
async fn index(state: &State<AppState>, handlebars: &State<Handlebars<'_>>) -> Html<String> {
    refresh_if_due(&*state).await;
    let data = state.data.read().await;
    let rendered_html = handlebars.render("dashboard", &*data).unwrap();
    Html(rendered_html)
}

#[rocket::main]
async fn main() {
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    handlebars
        .register_template_string("dashboard", include_str!("templates/index.hbs"))
        .unwrap();

    let rocket = rocket::build();
    let config: DashboardConfig = rocket
        .figment()
        .extract_inner("dashboard")
        .expect("Failed to load [dashboard] config from Rocket.toml");

    let shared_data = Arc::new(RwLock::new(DashboardData::default()));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(5000))
        .user_agent("LocalhostDashboard/1.0")
        .build()
        .expect("Failed to create reqwest client");

    let state = AppState {
        data: shared_data.clone(),
        refresh_ctx: Arc::new(Mutex::new(RefreshContext::default())),
        client,
        config: config.clone(),
    };

    let worker_state = state.clone();
    rocket::tokio::spawn(async move {
        loop {
            refresh_if_due(&worker_state).await;
            rocket::tokio::time::sleep(REFRESH_MIN_INTERVAL).await;
        }
    });

    let _result = rocket
        .mount("/", routes![index])
        .manage(handlebars)
        .manage(state)
        .launch()
        .await;
}
