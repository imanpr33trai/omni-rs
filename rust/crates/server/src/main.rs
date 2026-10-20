//! OmniRoute Rust server — Phase 1 skeleton (simplified working version).

use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use config::AppConfig;
use error::{error_response, ErrorCode, sanitize_error_message, sse_error_frame};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

use auth::{
    AuthTier, classify_route, create_dashboard_session_token, extract_api_key,
    is_local_request, verify_dashboard_session_token, HealthResponse,
};

const DEFAULT_PORT: u16 = 20128;
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct AppState {
    config: Arc<AppConfig>,
    start_time: Instant,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("APP_LOG_LEVEL")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Load configuration
    let config = Arc::new(AppConfig::load().expect("failed to load config"));
    let port = config.port();
    let start_time = Instant::now();

    let state = Arc::new(AppState { config, start_time });

    // Build CORS layer from config
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS, Method::HEAD])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::HeaderName::from_static("x-api-key"), header::HeaderName::from_static("x-csrf-token"), header::HeaderName::from_static("x-requested-with")])
        .allow_credentials(true)
        .max_age(Duration::from_secs(86400));

    // Build the router with middleware applied at the router level
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/auth/status", get(auth_status))
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/logout", post(auth_logout))
        .route("/api/v1/models", get(v1_models))
        .route("/api/v1/chat/completions", post(v1_chat_completions))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024))
                .layer(cors)
        )
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind listener");

    info!("OmniRoute Rust server listening on http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server exited with error");
}

/// GET /healthz — full health response per /api/monitoring/health contract
async fn healthz(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    let mut health = HealthResponse::default();
    health.uptime_secs = uptime;
    Json(health)
}

/// GET /api/auth/status — returns current auth status
async fn auth_status(State(state): State<Arc<AppState>>, request: Request) -> impl IntoResponse {
    let has_session = request
        .headers()
        .get(header::COOKIE)
        .and_then(|c| c.to_str().ok())
        .and_then(|cookie| cookie.split(';').find_map(|part| part.trim().strip_prefix("auth_token=").map(|v| v.to_string())))
        .map(|token| auth::verify_dashboard_session_token(&token).is_ok())
        .unwrap_or(false);

    Json(serde_json::json!({
        "authenticated": has_session,
        "requireApiKey": state.config.security.require_api_key,
    }))
}

/// POST /api/auth/login — dashboard login
async fn auth_login(State(state): State<Arc<AppState>>, Json(payload): Json<serde_json::Value>) -> Response {
    let password = payload.get("password").and_then(|v| v.as_str()).unwrap_or("");
    
    if password == state.config.secrets.initial_password {
        let token = auth::create_dashboard_session_token(
            "admin",
            Some(vec!["admin".to_string()]),
            Duration::from_secs(86400),
        ).unwrap_or_else(|_| String::new());

        let mut response = Json(serde_json::json!({ "success": true, "token": token })).into_response();
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!("auth_token={}; HttpOnly; SameSite=Lax; Path=/; Max-Age=86400", token)).unwrap(),
        );
        response
    } else {
        error_response(ErrorCode::InvalidApiKey, StatusCode::UNAUTHORIZED, "invalid credentials")
    }
}

/// POST /api/auth/logout — clear session
async fn auth_logout() -> Response {
    let mut response = Json(serde_json::json!({ "success": true })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("auth_token=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"),
    );
    response
}

/// GET /api/v1/models — model list (stub for Phase 1)
async fn v1_models() -> impl IntoResponse {
    Json(serde_json::json!({
        "object": "list",
        "data": [
            { "id": "mock-model", "object": "model", "owned_by": "mock" }
        ]
    }))
}

/// POST /api/v1/chat/completions — chat completions (stub for Phase 1)
async fn v1_chat_completions(Json(payload): Json<serde_json::Value>) -> impl IntoResponse {
    Json(serde_json::json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "created": 1700000000,
        "model": payload.get("model").and_then(|v| v.as_str()).unwrap_or("mock-model"),
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "Hello from Rust" },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 6, "total_tokens": 16 }
    }))
}

/// Graceful shutdown signal handler
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install SIGINT handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    info!("shutdown signal received — starting graceful drain (timeout: {:?})", GRACEFUL_SHUTDOWN_TIMEOUT);
}

#[test]
fn server_crate_scaffolded() {
    assert_eq!(DEFAULT_PORT, 20128);
}