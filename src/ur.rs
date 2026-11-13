use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use opentelemetry::KeyValue;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use crate::config::{CONFIG, EstateTarget};
use crate::metrics::{metrics, record_error};
use crate::supabase_store::SupabaseStore;

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct RoomListing {
    pub estate_name: String,
    pub room_name: String,
    pub commonfee: String,
    pub rent: String,
    pub layout: String,
    pub floorspace: String,
    pub floor: String,
}

#[derive(Debug, Deserialize)]
pub struct UrRoomApiResponse {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub rent: Option<String>,
    #[serde(default)]
    pub commonfee: Option<String>,
    #[serde(rename = "type", default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub floorspace: Option<String>,
    #[serde(default)]
    pub floor: Option<String>,
}

#[tracing::instrument(skip_all)]
async fn scan_cycle(
    estates: &[EstateTarget],
    client: &reqwest::Client,
    store: &SupabaseStore,
    notified: &mut HashSet<RoomListing>,
) {
    let start = Instant::now();
    let outcome = match scan_estates(estates, client, store, notified).await {
        Ok(()) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            metrics().last_success.record(now, &[]);
            "ok"
        }
        Err(e) => {
            record_error("scan", &e);
            "error"
        }
    };
    let m = metrics();
    m.scan_cycles.add(1, &[KeyValue::new("outcome", outcome)]);
    m.scan_duration.record(start.elapsed().as_secs_f64(), &[]);
}

pub fn load_estates() -> Result<Vec<EstateTarget>> {
    let path = &CONFIG.estates_config_path;
    debug!("Path resolved to: {}", path);
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config at '{}'", path))?;
    debug!("File read ({} bytes)", content.len());

    serde_yml::from_str(&content)
        .with_context(|| format!("Failed to parse YAML config at '{}'", path))
}

async fn scan_estates(
    estates: &[EstateTarget],
    client: &reqwest::Client,
    store: &SupabaseStore,
    notified: &mut HashSet<RoomListing>,
) -> Result<()> {
    debug!("Start scanning for {:?}.", estates);
    let mut current_vacancies = HashSet::new();

    for target in estates {
        let mut form = HashMap::new();
        form.insert("shisya", target.shisya.as_str());
        form.insert("danchi", target.danchi.as_str());
        form.insert("shikibetu", target.shikibetu.as_str());
        form.insert("orderByField", "0");
        form.insert("orderBySort", "0");
        form.insert("pageIndex", "0");
        info!("Request for {}", target.name);

        let resp = client
            .post(&CONFIG.ur_api_url)
            .form(&form)
            .send()
            .await
            .with_context(|| format!("HTTP request failed for {}", target.name))?;

        let status = resp.status();
        let class = match status.as_u16() {
            200..=299 => "2xx",
            400..=499 => "4xx",
            _ => "5xx",
        };
        metrics()
            .http_requests
            .add(1, &[KeyValue::new("status_class", class)]);

        let resp = resp
            .error_for_status()
            .with_context(|| format!("UR API returned HTTP {} for {}", status, target.name))?;

        let rooms: Option<Vec<UrRoomApiResponse>> = resp
            .json()
            .await
            .with_context(|| format!("Failed to decode JSON response for {}", target.name))?;

        match rooms {
            Some(list) if !list.is_empty() => {
                info!("[+] {}: Found {} unit(s)", target.name, list.len());
                for room in list {
                    let vacancy = RoomListing {
                        estate_name: target.name.clone(),
                        room_name: room.name.unwrap_or_else(|| "N/A".into()),
                        commonfee: room.commonfee.unwrap_or_else(|| "N/A".into()),
                        rent: room.rent.unwrap_or_else(|| "N/A".into()),
                        layout: room.layout.unwrap_or_else(|| "N/A".into()),
                        floorspace: room.floorspace.unwrap_or_else(|| "N/A".into()),
                        floor: room.floor.unwrap_or_else(|| "N/A".into()),
                    };

                    current_vacancies.insert(vacancy.clone());

                    if notified.contains(&vacancy) {
                        debug!("Skipping already-notified room: {:?}", vacancy);
                        continue;
                    }

                    if let Err(err) = notify(client, &format!("room: {:?}", vacancy)).await {
                        error!("Failed to notify for {:?}: {:#}", vacancy, err);
                    } else {
                        if let Err(err) = store.mark_notified(&vacancy).await {
                            error!(
                                "Notified user, but failed to persist {:?} to Supabase: {:#}",
                                vacancy, err
                            );
                        }
                        notified.insert(vacancy);
                    }
                }
            }
            _ => {
                info!("[-] {}: 0 vacancies", target.name);
            }
        }
    }

    // Prune rooms no longer listed on UR so they trigger alerts if re-listed later
    let expired: Vec<RoomListing> = notified
        .iter()
        .filter(|room| !current_vacancies.contains(*room))
        .cloned()
        .collect();

    for room in expired {
        if let Err(err) = store.remove_unlisted(&room).await {
            error!(
                "Failed to delete expired room {:?} from Supabase: {:#}",
                room, err
            );
        } else {
            notified.remove(&room);
        }
    }

    Ok(())
}

pub async fn notify(client: &reqwest::Client, msg: &str) -> anyhow::Result<()> {
    info!("room: {:?}", msg);
    client
        .post(&CONFIG.notification_url)
        .body(msg.to_owned())
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}

#[tracing::instrument]
pub async fn launch(mut shutdown_rx: tokio::sync::watch::Receiver<bool>) -> anyhow::Result<()> {
    debug!("Launching.");
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
    );
    headers.insert(ORIGIN, HeaderValue::from_static("https://www.ur-net.go.jp"));
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://www.ur-net.go.jp/"),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/javascript, */*; q=0.01"),
    );
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded; charset=UTF-8"),
    );

    let http_client = reqwest::Client::builder()
        .default_headers(headers)
        .build()?;

    let store = SupabaseStore::from_config()?;
    let mut notified = match store.load_notified().await {
        Ok(set) => {
            info!(
                "Loaded {} previously notified vacancies from Supabase.",
                set.len()
            );
            set
        }
        Err(err) => {
            error!(
                "Failed to load initial state from Supabase (starting empty): {:#}",
                err
            );
            HashSet::new()
        }
    };

    loop {
        info!("Load estates.");
        let estates = match load_estates() {
            Ok(estates) => estates,
            Err(err) => {
                error!("Load estates failed: {:#}", err);
                break;
            }
        };

        info!("Scan estates.");
        scan_cycle(&estates, &http_client, &store, &mut notified).await;

        info!(
            "Scanning estate done. Active notified count: {}",
            notified.len()
        );
        // 3. INTERRUPTIBLE SLEEP: Races the 600s timer against the shutdown signal
        tokio::select! {
            _ = generate_periodic_heartbeat_logs() => {}
            _ = shutdown_rx.changed() => {
                // SIGTERM or Ctrl+C arrived: `sleep(600s)` is dropped immediately!
                info!("Shutdown signal received during 600s sleep; exiting launch cleanly.");
                break;
            }
        }
    }

    info!("Scanning done.");
    Ok(())
}

async fn generate_periodic_heartbeat_logs() {
    info!("Scanning estate done. Sleeping for 600s...");

    let total_wait = Duration::from_secs(600);
    let tick = Duration::from_secs(60);
    let mut elapsed = Duration::ZERO;

    while elapsed < total_wait {
        let step = tick.min(total_wait - elapsed);
        tokio::time::sleep(step).await;
        elapsed += step;

        let remaining = total_wait.saturating_sub(elapsed).as_secs();
        if remaining > 0 {
            info!(remaining_secs = remaining, "Waiting for next scan...");
        }
    }
}
