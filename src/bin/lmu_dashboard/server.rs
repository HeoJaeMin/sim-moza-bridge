use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::{RwLock, mpsc, oneshot, watch};

use crate::model::{ClassLeaderIdentity, LiveSnapshot, TraceResponse};
use crate::store::DashboardStore;

const INDEX_HTML: &str = include_str!("assets/index.html");
const APP_JS: &str = include_str!("assets/app.js");
const STYLES_CSS: &str = include_str!("assets/styles.css");
const CONTROL_HEADER: &str = "x-lmu-dashboard-control";
const CONTROL_HEADER_VALUE: &str = "1";

#[derive(Clone)]
pub struct DashboardState {
    live: Arc<RwLock<LiveSnapshot>>,
    trace: Arc<RwLock<TraceResponse>>,
    analysis: Arc<RwLock<Value>>,
    store: DashboardStore,
    control: Option<mpsc::UnboundedSender<CollectorCommand>>,
    shutdown: Option<watch::Sender<bool>>,
}

#[derive(Debug)]
pub enum CollectorCommand {
    Pause(oneshot::Sender<Result<(), String>>),
    Resume(oneshot::Sender<Result<(), String>>),
    Shutdown(oneshot::Sender<Result<(), String>>),
}

impl DashboardState {
    pub fn new(store: DashboardStore) -> Self {
        Self {
            live: Arc::new(RwLock::new(LiveSnapshot::default())),
            trace: Arc::new(RwLock::new(TraceResponse::default())),
            analysis: Arc::new(RwLock::new(serde_json::json!({
                "schema_version": 3,
                "status": "waiting_for_session",
                "message": "LMU 세션을 기다리고 있습니다."
            }))),
            store,
            control: None,
            shutdown: None,
        }
    }

    pub fn with_control(
        mut self,
        control: mpsc::UnboundedSender<CollectorCommand>,
        shutdown: watch::Sender<bool>,
    ) -> Self {
        self.control = Some(control);
        self.shutdown = Some(shutdown);
        self
    }

    pub async fn publish(&self, live: LiveSnapshot, trace: TraceResponse) {
        *self.live.write().await = live;
        *self.trace.write().await = trace;
    }

    pub async fn publish_analysis(&self, analysis: Value) {
        *self.analysis.write().await = analysis;
    }

    pub async fn class_leader(&self) -> Option<ClassLeaderIdentity> {
        let live = self.live.read().await;
        class_leader_identity(&live)
    }
}

fn class_leader_identity(live: &LiveSnapshot) -> Option<ClassLeaderIdentity> {
    let session_id = live.session.as_ref()?.id.trim();
    if session_id.is_empty() {
        return None;
    }
    let player = live
        .vehicles
        .iter()
        .find(|vehicle| vehicle.is_player)
        .or_else(|| {
            let player_id = live.player.as_ref()?.vehicle_id;
            live.vehicles.iter().find(|vehicle| vehicle.id == player_id)
        })?;
    let player_class = player.class_name.trim();
    if player_class.is_empty() {
        return None;
    }
    let leader = live
        .vehicles
        .iter()
        .filter(|vehicle| {
            vehicle.position > 0 && vehicle.class_name.trim().eq_ignore_ascii_case(player_class)
        })
        .min_by_key(|vehicle| vehicle.position)?;
    Some(ClassLeaderIdentity {
        session_id: session_id.to_owned(),
        vehicle_id: leader.id,
        driver_name: leader.driver_name.clone(),
        class_name: leader.class_name.clone(),
        player_vehicle_id: player.id,
        player_driver_name: player.driver_name.clone(),
    })
}

pub async fn serve(
    address: SocketAddr,
    state: DashboardState,
    shutdown: watch::Receiver<bool>,
) -> Result<(), String> {
    let listener = TcpListener::bind(address)
        .await
        .map_err(|error| format!("failed to bind dashboard to {address}: {error}"))?;
    serve_listener(listener, state, shutdown).await
}

pub async fn serve_listener(
    listener: TcpListener,
    state: DashboardState,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), String> {
    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/styles.css", get(styles_css))
        .route("/api/live", get(live))
        .route("/api/trace", get(trace))
        .route("/api/laps", get(laps))
        .route("/api/laps/{id}", get(lap))
        .route("/api/contacts", get(contacts))
        .route("/api/analysis", get(analysis))
        .route("/api/control/{action}", post(control))
        .route("/api/health", get(health))
        .with_state(state)
        .layer(middleware::map_response(json_utf8_content_type));
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        while !*shutdown.borrow() {
            if shutdown.changed().await.is_err() {
                break;
            }
        }
    })
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
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.list_laps())
        .await
        .map_err(join_error)?
        .map(Json)
        .map_err(internal_error)
}

async fn lap(
    State(state): State<DashboardState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    if !valid_id(&id) {
        return Err((StatusCode::BAD_REQUEST, "invalid lap id".to_owned()));
    }
    let store = state.store.clone();
    let id_for_query = id.clone();
    let saved = tokio::task::spawn_blocking(move || store.load_lap(&id_for_query))
        .await
        .map_err(join_error)?
        .map_err(internal_error)?;
    match saved {
        Some(lap) => Ok(Json(lap).into_response()),
        None => Ok((StatusCode::NOT_FOUND, "lap not found").into_response()),
    }
}

async fn contacts(
    State(state): State<DashboardState>,
) -> Result<Json<Vec<crate::model::ContactEvent>>, (StatusCode, String)> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.recent_contacts(100))
        .await
        .map_err(join_error)?
        .map(Json)
        .map_err(internal_error)
}

async fn analysis(State(state): State<DashboardState>) -> Json<Value> {
    Json(state.analysis.read().await.clone())
}

async fn control(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<DashboardState>,
    Path(action): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !peer.ip().is_loopback() {
        return Err((
            StatusCode::FORBIDDEN,
            "dashboard control is available only from this PC".to_owned(),
        ));
    }
    if headers
        .get(CONTROL_HEADER)
        .and_then(|value| value.to_str().ok())
        != Some(CONTROL_HEADER_VALUE)
    {
        return Err((
            StatusCode::FORBIDDEN,
            "dashboard control request header is missing".to_owned(),
        ));
    }
    let (acknowledge, response) = oneshot::channel();
    let (command, shutdown_requested) = match action.as_str() {
        "pause" => (CollectorCommand::Pause(acknowledge), false),
        "resume" => (CollectorCommand::Resume(acknowledge), false),
        "shutdown" => (CollectorCommand::Shutdown(acknowledge), true),
        _ => return Err((StatusCode::NOT_FOUND, "unknown control action".to_owned())),
    };
    state
        .control
        .as_ref()
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "collector control is unavailable".to_owned(),
            )
        })?
        .send(command)
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "collector has already stopped".to_owned(),
            )
        })?;
    response
        .await
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "collector stopped before acknowledging control".to_owned(),
            )
        })?
        .map_err(internal_error)?;
    if shutdown_requested && let Some(shutdown) = &state.shutdown {
        let _ = shutdown.send(true);
    }
    Ok(Json(serde_json::json!({
        "ok": true,
        "action": action,
    })))
}

async fn health() -> &'static str {
    "ok"
}

async fn json_utf8_content_type(mut response: Response) -> Response {
    let is_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(';').next().is_some_and(|media_type| {
                media_type.trim().eq_ignore_ascii_case("application/json")
            })
        });
    if is_json {
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
    }
    response
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

fn join_error(error: tokio::task::JoinError) -> (StatusCode, String) {
    internal_error(format!("database worker stopped unexpectedly: {error}"))
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

    #[test]
    fn bundled_ui_separates_capture_counters_and_sends_the_control_header() {
        for id in [
            "capture-accepted",
            "capture-rejected",
            "capture-duplicates",
            "capture-stalled",
            "capture-inconsistent",
            "capture-invalid-sessions",
            "capture-telemetry-accepted",
            "capture-telemetry-rejected",
            "capture-telemetry-duplicates",
            "capture-telemetry-backward",
            "capture-telemetry-delayed",
            "capture-telemetry-sudden",
        ] {
            assert!(INDEX_HTML.contains(id));
        }
        assert!(!APP_JS.contains("rejected + invalidSession"));
        assert!(APP_JS.contains("X-LMU-Dashboard-Control"));
    }

    #[test]
    fn derives_the_live_leader_from_the_players_class() {
        let live = LiveSnapshot {
            session: Some(crate::model::SessionState {
                id: "session".to_owned(),
                ..crate::model::SessionState::default()
            }),
            vehicles: vec![
                crate::model::VehicleState {
                    id: 1,
                    driver_name: "전체 선두".to_owned(),
                    class_name: "LMGT3".to_owned(),
                    position: 1,
                    ..crate::model::VehicleState::default()
                },
                crate::model::VehicleState {
                    id: 7,
                    driver_name: "플레이어".to_owned(),
                    class_name: "Hypercar".to_owned(),
                    position: 4,
                    is_player: true,
                    ..crate::model::VehicleState::default()
                },
                crate::model::VehicleState {
                    id: 8,
                    driver_name: "클래스 선두".to_owned(),
                    class_name: "hypercar".to_owned(),
                    position: 2,
                    ..crate::model::VehicleState::default()
                },
            ],
            ..LiveSnapshot::default()
        };

        let leader = class_leader_identity(&live).unwrap();

        assert_eq!(leader.session_id, "session");
        assert_eq!(leader.vehicle_id, 8);
        assert_eq!(leader.driver_name, "클래스 선두");
        assert_eq!(leader.player_vehicle_id, 7);
        assert_eq!(leader.player_driver_name, "플레이어");
    }

    #[tokio::test]
    async fn accepts_local_controls_and_rejects_remote_controls() {
        let directory = std::env::temp_dir().join(format!(
            "lmu-dashboard-control-test-{}",
            crate::store::unix_ms()
        ));
        let store = DashboardStore::open(&directory).unwrap();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let (shutdown, shutdown_rx) = watch::channel(false);
        let state = DashboardState::new(store).with_control(sender, shutdown);
        let mut headers = HeaderMap::new();
        headers.insert(CONTROL_HEADER, CONTROL_HEADER_VALUE.parse().unwrap());

        let missing_header = control(
            ConnectInfo("127.0.0.1:50000".parse().unwrap()),
            State(state.clone()),
            Path("pause".to_owned()),
            HeaderMap::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(missing_header.0, StatusCode::FORBIDDEN);

        let pause = tokio::spawn(control(
            ConnectInfo("127.0.0.1:50000".parse().unwrap()),
            State(state.clone()),
            Path("pause".to_owned()),
            headers.clone(),
        ));
        let Some(CollectorCommand::Pause(acknowledge)) = receiver.recv().await else {
            panic!("expected pause command");
        };
        assert!(!pause.is_finished());
        acknowledge.send(Ok(())).unwrap();
        let _ = pause.await.unwrap().unwrap();

        let remote = control(
            ConnectInfo("192.168.1.20:50000".parse().unwrap()),
            State(state.clone()),
            Path("resume".to_owned()),
            headers.clone(),
        )
        .await
        .unwrap_err();
        assert_eq!(remote.0, StatusCode::FORBIDDEN);

        let failed_resume = tokio::spawn(control(
            ConnectInfo("127.0.0.1:50000".parse().unwrap()),
            State(state.clone()),
            Path("resume".to_owned()),
            headers.clone(),
        ));
        let Some(CollectorCommand::Resume(acknowledge)) = receiver.recv().await else {
            panic!("expected resume command");
        };
        acknowledge
            .send(Err("failed to apply resume".to_owned()))
            .unwrap();
        let error = failed_resume.await.unwrap().unwrap_err();
        assert_eq!(error.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(error.1.contains("failed to apply resume"));

        let shutdown_request = tokio::spawn(control(
            ConnectInfo("[::1]:50000".parse().unwrap()),
            State(state),
            Path("shutdown".to_owned()),
            headers,
        ));
        let Some(CollectorCommand::Shutdown(acknowledge)) = receiver.recv().await else {
            panic!("expected shutdown command");
        };
        assert!(!*shutdown_rx.borrow());
        acknowledge.send(Ok(())).unwrap();
        let _ = shutdown_request.await.unwrap().unwrap();
        assert!(*shutdown_rx.borrow());
        std::fs::remove_dir_all(directory).ok();
    }

    #[tokio::test]
    async fn serves_non_ascii_json_with_an_explicit_utf8_content_type() {
        let directory = std::env::temp_dir().join(format!(
            "lmu-dashboard-utf8-http-test-{}",
            crate::store::unix_ms()
        ));
        let store = DashboardStore::open(&directory).unwrap();
        let state = DashboardState::new(store);
        state
            .publish_analysis(serde_json::json!({
                "message": "한글 코칭 🏁",
                "driver": "드라이버 é"
            }))
            .await;
        let response = json_utf8_content_type(analysis(State(state)).await.into_response()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json; charset=utf-8"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["message"], "한글 코칭 🏁");
        assert_eq!(payload["driver"], "드라이버 é");

        std::fs::remove_dir_all(directory).ok();
    }
}
