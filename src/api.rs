use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router
};
use std::time::Duration;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::{info, instrument, Level};
// use tokio::sync::watch;

#[derive(Debug)]
enum AppError {
    NotFound(u32),
    Internal(&'static str),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "item {id} not found"),
            Self::Internal(msg) => write!(f, "{msg}"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, self.to_string()).into_response()
    }
}

async fn health() -> &'static str {
    "ok"
}

#[instrument]
async fn get_item(Path(id): Path<u32>) -> Result<String, AppError> {
    info!(item_id = id, "looking up item");
    tokio::time::sleep(Duration::from_millis(50)).await;
    if id == 0 {
        return Err(AppError::NotFound(id)); // 404
    }
    Ok(format!("item {id}\n"))
}

#[instrument(err)] // logs the error inside this child span
async fn query_database() -> Result<(), AppError> {
    tokio::time::sleep(Duration::from_millis(30)).await;
    Err(AppError::Internal("database connection refused (simulated)"))
}

#[instrument]
async fn fail() -> Result<String, AppError> {
    info!("about to fail");
    query_database().await?; // 500
    Ok("unreachable\n".to_string())
}

fn router() -> Router {
    // Everything except /health is traced, so probes don't flood Tempo.
    let traced = Router::new()
        .route("/items/{id}", get(get_item))
        .route("/fail", get(fail))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        );

    Router::new().route("/health", get(health)).merge(traced)
}

pub async fn serve(addr: String, mut shutdown: tokio::sync::watch::Receiver<bool>) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("api listening @{}", &addr);
    axum::serve(listener, router())
        .with_graceful_shutdown(async move {
            let _ = shutdown.changed().await;
        })
        .await?;
    Ok(())
}
