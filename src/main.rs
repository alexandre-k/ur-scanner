use anyhow::{anyhow, Result};
use std::{sync::LazyLock};
use tracing::{error, info};
use tokio::sync::watch;

mod api;
mod config;
mod metrics;
mod supabase_store;
mod telemetry;
mod ur;

async fn shutdown_signal() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = term.recv() => {},
    }
    info!("shutdown signal received");
}


async fn run() -> Result<()> {
    // let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let addr = format!("{}:{}", config::CONFIG.app_url, config::CONFIG.app_port);
    let stop_api_rx = shutdown_rx.clone();
    let mut api_daemon = tokio::spawn(api::serve(addr, stop_api_rx));

    // let mut api_done = false;
    // let mut scanner_done = false;
    // 2. Spawn the scanner loop onto its own task so `select!` cannot drop it mid-cycle
    let mut scanner = tokio::spawn(ur::launch(shutdown_rx));

let result: anyhow::Result<()> = tokio::select! {
        r = &mut scanner => {
            // scanner_done = true;
            match r {
                Ok(res) => { info!("scanner finished"); res }
                Err(e) => Err(e.into()),
            }
        }
        r = &mut api_daemon => {
            match r {
                Ok(Ok(())) => Err(anyhow!("api stopped unexpectedly")),
                Ok(Err(e)) => { error!(error = %e, "api failed"); Err(e) }
                Err(e) => Err(e.into()),
            }
        }
        _ = shutdown_signal() => Ok(()),
    };

    // 3. Notify BOTH the API and the scanner to shut down
    let _ = shutdown_tx.send(true);
    // Stop the API and give in-flight requests a few seconds to finish.
    // if !api_done {
    //     let _ = stop_tx.send(());
        // let _ = tokio::time::timeout(Duration::from_secs(5), api).await;
    // }
    result
}

fn main() -> Result<()> {
    LazyLock::force(&config::CONFIG);
    let tel = telemetry::init("ur-scanner")?;

    info!("Starting UR scanner.");
    info!(version = env!("CARGO_PKG_VERSION"), "starting");

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;

    let result = rt.block_on(run());

    // Shut down AFTER block_on returns, outside the async context. This also flushes the last batch.
    tel.shutdown();
    result
}
