#[macro_use]
extern crate rocket;

use handlebars::Handlebars;
use listeners::get_all;
use rocket::response::content::RawHtml;
use rocket::response::Redirect;
use rocket::serde::json::Json;
use rocket::tokio::sync::RwLock;
use rocket::State;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{Pid, System};

#[derive(Deserialize, Clone)]
struct DashboardConfig {
    ignored_processes: Vec<String>,
    dev_port_min: u16,
    dev_port_max: u16,
}

#[derive(Clone, Serialize)]
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

const REFRESH_INTERVAL: Duration = Duration::from_secs(10);

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
    extract_title(&body)
}

// ponytail: string scan instead of an HTML parser; entities stay encoded and
// titles containing markup aren't handled — swap in an HTML parser crate if that ever matters.
fn extract_title(body: &str) -> Option<String> {
    let lower = body.to_lowercase();
    let tag = lower.find("<title")?;
    let start = tag + lower[tag..].find('>')? + 1;
    let end = start + lower[start..].find("</title>")?;
    let title = body.get(start..end)?.trim();
    if title.is_empty() {
        return None;
    }
    Some(title.to_string())
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

    let titles = rocket::futures::future::join_all(title_futures).await;
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

        let service = is_service(&info.name, info.port);
        if is_dev_port(info.port, config) && !service {
            local_apps.push(info);
        } else if service || info.port < config.dev_port_min {
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

#[cfg(test)]
mod tests {
    use super::extract_title;

    #[test]
    fn extracts_title() {
        assert_eq!(
            extract_title("<html><head><TITLE> My App </TITLE></head>"),
            Some("My App".to_string())
        );
        assert_eq!(extract_title("<title lang=\"en\">x</title>"), Some("x".to_string()));
        assert_eq!(extract_title("<title></title>"), None);
        assert_eq!(extract_title("no title here"), None);
    }
}

#[get("/")]
async fn index(
    data: &State<Arc<RwLock<DashboardData>>>,
    handlebars: &State<Handlebars<'_>>,
) -> RawHtml<String> {
    let data = data.read().await;
    let rendered_html = handlebars.render("dashboard", &*data).unwrap();
    RawHtml(rendered_html)
}

#[get("/json")]
async fn json(data: &State<Arc<RwLock<DashboardData>>>) -> Json<DashboardData> {
    Json(data.read().await.clone())
}

// ponytail: kills any pid, no ownership check — localhost-only single-user tool.
// The killed process stays listed until the next 10s refresh.
#[post("/kill/<pid>")]
async fn kill(pid: u32) -> Redirect {
    let _ = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status();
    Redirect::to("/")
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

    let worker_data = shared_data.clone();
    rocket::tokio::spawn(async move {
        let mut title_by_addr = HashMap::new();
        let mut title_attempted = HashSet::new();
        loop {
            let new_data =
                refresh_dashboard_data(&client, &config, &mut title_by_addr, &mut title_attempted)
                    .await;
            *worker_data.write().await = new_data;
            rocket::tokio::time::sleep(REFRESH_INTERVAL).await;
        }
    });

    let _result = rocket
        .mount("/", routes![index, json, kill])
        .manage(handlebars)
        .manage(shared_data)
        .launch()
        .await;
}
