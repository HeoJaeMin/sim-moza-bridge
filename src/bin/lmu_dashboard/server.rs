use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use crate::model::{LiveSnapshot, TraceResponse};
use crate::store::DashboardStore;

const INDEX_HTML: &str = include_str!("assets/index.html");
const APP_JS: &str = include_str!("assets/app.js");
const STYLES_CSS: &str = include_str!("assets/styles.css");

#[derive(Clone)]
pub struct DashboardState {
    live: Arc<RwLock<LiveSnapshot>>,
    trace: Arc<RwLock<TraceResponse>>,
    store: DashboardStore,
}

impl DashboardState {
    pub fn new(store: DashboardStore) -> Self {
        Self {
            live: Arc::new(RwLock::new(LiveSnapshot::default())),
            trace: Arc::new(RwLock::new(TraceResponse::default())),
            store,
        }
    }

    pub async fn publish(&self, live: LiveSnapshot, trace: TraceResponse) {
        *self.live.write().await = live;
        *self.trace.write().await = trace;
    }
}

pub async fn serve(address: SocketAddr, state: DashboardState) -> Result<(), String> {
    let listener = TcpListener::bind(address)
        .await
        .map_err(|error| format!("failed to bind dashboard to {address}: {error}"))?;
    serve_listener(listener, state).await
}

pub async fn serve_listener(listener: TcpListener, state: DashboardState) -> Result<(), String> {
    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/styles.css", get(styles_css))
        .route("/api/live", get(live))
        .route("/api/trace", get(trace))
        .route("/api/laps", get(laps))
        .route("/api/laps/{id}", get(lap))
        .route("/api/contacts", get(contacts))
        .route("/api/health", get(health))
        .with_state(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| format!("dashboard server failed: {error}"))
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn app_js() -> Response {
    static_asset(APP_JS, "application/javascript; charset=utf-8")
}

async fn styles_css() -> Response {
    static_asset(STYLES_CSS, "text/css; charset=utf-8")
}

async fn live(State(state): State<DashboardState>) -> Json<LiveSnapshot> {
    Json(state.live.read().await.clone())
}

async fn trace(State(state): State<DashboardState>) -> Json<TraceResponse> {
    Json(state.trace.read().await.clone())
}

async fn laps(
    State(state): State<DashboardState>,
) -> Result<Json<Vec<crate::model::LapSummary>>, (StatusCode, String)> {
    state.store.list_laps().map(Json).map_err(internal_error)
}

async fn lap(
    State(state): State<DashboardState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    if !valid_id(&id) {
        return Err((StatusCode::BAD_REQUEST, "invalid lap id".to_owned()));
    }
    match state.store.load_lap(&id).map_err(internal_error)? {
        Some(lap) => Ok(Json(lap).into_response()),
        None => Ok((StatusCode::NOT_FOUND, "lap not found").into_response()),
    }
}

async fn contacts(
    State(state): State<DashboardState>,
) -> Result<Json<Vec<crate::model::ContactEvent>>, (StatusCode, String)> {
    state
        .store
        .recent_contacts(100)
        .map(Json)
        .map_err(internal_error)
}

async fn health() -> &'static str {
    "ok"
}

fn static_asset(body: &'static str, content_type: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn internal_error(error: String) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error)
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_lap_ids_without_accepting_paths() {
        assert!(valid_id("session-lap-12-123"));
        assert!(!valid_id("../dashboard.sqlite3"));
        assert!(!valid_id("lap/12"));
    }
}
