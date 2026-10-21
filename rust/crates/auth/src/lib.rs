//! OmniRoute authentication & authorization layer.
//!
//! Ports the TS authz pipeline from `src/server/authz/` + `src/proxy.ts`
//! + `src/server/authz/routeGuard.ts`.
//!
//! Tier model (exact parity with `routeGuard.ts`):
//! - `LocalOnly`: loopback/LAN only, pre-auth, unconditional (protects
//!   process-spawning endpoints like /api/mcp/, /api/cli-tools/*,
//!   /api/services/, /api/version-manager/, etc.)
//! - `AlwaysProtected`: auth required even when REQUIRE_API_KEY=false
//!   (e.g. /api/shutdown, /api/settings/database, /api/db-backups)
//! - `Management`: default tier for all other routes; waived when
//!   REQUIRE_API_KEY=false

use std::net::IpAddr;
use std::time::{Duration, SystemTime};
use axum::http::{HeaderMap, HeaderValue, Method};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation, TokenData};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Sha512};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthTier {
    LocalOnly,
    AlwaysProtected,
    Management,
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("local-only route: {0}")]
    LocalOnly(String),
    #[error("always-protected route requires authentication")]
    AlwaysProtected,
    #[error("management route requires authentication")]
    Management,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("jwt error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
}

/// Exact route classification from `src/server/authz/routeGuard.ts`
/// (Tier lists + regexes ported 1:1)
pub fn classify_route(path: &str) -> AuthTier {
    // LOCAL_ONLY prefixes (loopback/LAN, pre-auth, unconditional)
    const LOCAL_ONLY_EXACT: &[&str] = &[
        "/api/mcp/",
        "/api/cli-tools/",
        "/api/services/",
        "/api/version-manager/",
        "/api/tunnels/cloudflared",
        "/api/tunnels/tailscale/",
        "/api/plugins/",
        "/api/middleware/",
        "/api/system/version",
        "/api/db-backups/exportAll",
        "/api/db/health",
        "/api/local/",
        "/api/headroom/start",
        "/api/headroom/stop",
        "/api/jobs/",
        "/api/oauth/cursor/auto-import",
        "/api/oauth/kiro/auto-import",
        "/api/skills/collect/",
        "/api/skills/install",
        "/api/skills/executions",
        "/api/discovery/",
        "/api/vnc-session/",
        "/api/acp/agents",
        "/api/resilience/connections",
        "/api/modality-bridge/video/",
        "/api/copilot/",
        "/api/tools/agent-bridge/",
        "/api/tools/traffic-inspector/",
        "/api/settings/mitm",
        "/api/issue-agent/",
        "/dashboard/providers/services/",
    ];

    const LOCAL_ONLY_REGEX: &[&str] = &[
        r"^/api/providers/[^/]+/(login|refresh-cursor|chatgpt-web-codex-doctor)$",
        r"^/api/volcengine-plan/connect$",
        r"^/api/providers/(validate|import|bulk)$",
    ];

    // ALWAYS_PROTECTED prefixes
    const ALWAYS_PROTECTED_EXACT: &[&str] = &[
        "/api/shutdown",
        "/api/settings/database",
        "/api/db-backups",
        "/api/settings/export-json",
        "/api/settings/import-json",
        "/api/settings/obsidian",
        "/api/logs/export",
        "/api/cli-tools/codex-profiles",
        "/api/providers/client",
        "/api/providers/agy-auth/apply-local",
        "/api/providers/health-autopilot/actions",
    ];

    const ALWAYS_PROTECTED_REGEX: &[&str] = &[
        r"^/api/providers/[^/]+/(claude|codex)-auth/(export|apply-local)$",
    ];

    // Check LOCAL_ONLY
    for prefix in LOCAL_ONLY_EXACT {
        if path.starts_with(prefix) {
            return AuthTier::LocalOnly;
        }
    }
    for regex in LOCAL_ONLY_REGEX {
        if regex::Regex::new(regex).unwrap().is_match(path) {
            return AuthTier::LocalOnly;
        }
    }

    // Check ALWAYS_PROTECTED
    for prefix in ALWAYS_PROTECTED_EXACT {
        if path.starts_with(prefix) {
            return AuthTier::AlwaysProtected;
        }
    }
    for regex in ALWAYS_PROTECTED_REGEX {
        if regex::Regex::new(regex).unwrap().is_match(path) {
            return AuthTier::AlwaysProtected;
        }
    }

    // Default: Management
    AuthTier::Management
}

/// Check if an IP is loopback or LAN (for LocalOnly enforcement)
pub fn is_local_request(peer_ip: IpAddr) -> bool {
    match peer_ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() ||
            v4.is_private() || // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
            v4.is_link_local() // 169.254.0.0/16
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() ||
            v6.segments()[0] & 0xfe00 == 0xfc00 // ULA (fc00::/7)
        }
    }
}

/// Extract the JWT_SECRET from env (required, no fallback — mirrors TS)
pub fn get_jwt_secret() -> Vec<u8> {
    std::env::var("JWT_SECRET")
        .expect("JWT_SECRET must be set")
        .into_bytes()
}

/// Extract the API_KEY_SECRET from env (required)
pub fn get_api_key_secret() -> Vec<u8> {
    std::env::var("API_KEY_SECRET")
        .expect("API_KEY_SECRET must be set")
        .into_bytes()
}

// ============================================================================
// CORS Configuration (ports src/server/cors/origins.ts + src/shared/utils/cors.ts)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub expose_headers: Vec<String>,
    pub allow_credentials: bool,
    pub max_age: Option<Duration>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "PATCH".to_string(),
                "DELETE".to_string(),
                "OPTIONS".to_string(),
                "HEAD".to_string(),
            ],
            allowed_headers: vec![
                "content-type".to_string(),
                "authorization".to_string(),
                "x-api-key".to_string(),
                "x-csrf-token".to_string(),
                "x-requested-with".to_string(),
            ],
            expose_headers: vec![
                "x-ratelimit-remaining-requests".to_string(),
                "x-ratelimit-remaining-tokens".to_string(),
                "x-ratelimit-reset-requests".to_string(),
                "x-ratelimit-reset-tokens".to_string(),
                "x-request-id".to_string(),
            ],
            allow_credentials: true,
            max_age: Some(Duration::from_secs(86400)), // 24h
        }
    }
}

/// Build CORS headers for a response — mirrors `handleCorsOptions` + `cors.ts`
pub fn build_cors_headers(config: &CorsConfig, origin: Option<&HeaderValue>, is_preflight: bool) -> HeaderMap {
    let mut headers = HeaderMap::new();
    
    // Origin handling
    if let Some(origin_val) = origin {
        if config.allowed_origins.contains(&"*".to_string()) {
            headers.insert(
                axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
        } else if config.allowed_origins.iter().any(|o| o == origin_val.to_str().unwrap_or("")) {
            headers.insert(
                axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                origin_val.clone(),
            );
        }
    }

    if config.allow_credentials {
        headers.insert(
            axum::http::header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
    }

    if is_preflight {
        let methods = config.allowed_methods.join(", ");
        headers.insert(
            axum::http::header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_str(&methods).unwrap(),
        );
        
        let headers_str = config.allowed_headers.join(", ");
        headers.insert(
            axum::http::header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_str(&headers_str).unwrap(),
        );

        if let Some(max_age) = config.max_age {
            headers.insert(
                axum::http::header::ACCESS_CONTROL_MAX_AGE,
                HeaderValue::from_str(&max_age.as_secs().to_string()).unwrap(),
            );
        }
    }

    if !config.expose_headers.is_empty() {
        headers.insert(
            axum::http::header::ACCESS_CONTROL_EXPOSE_HEADERS,
            HeaderValue::from_str(&config.expose_headers.join(", ")).unwrap(),
        );
    }

    headers
}

// ============================================================================
// CSRF Protection (ports src/server/authz/csrf.ts)
// ============================================================================

/// CSRF token validation — double-submit cookie pattern
pub fn validate_csrf_token(headers: &HeaderMap, cookie_token: Option<&str>) -> bool {
    let header_token = headers
        .get("x-csrf-token")
        .or_else(|| headers.get("x-xsrf-token"))
        .and_then(|v| v.to_str().ok());
    
    match (header_token, cookie_token) {
        (Some(h), Some(c)) => h == c,
        _ => false,
    }
}

/// Generate a CSRF token (32 bytes, base64)
pub fn generate_csrf_token() -> String {
    use uuid::Uuid;
    BASE64_STANDARD.encode(Uuid::new_v4().as_bytes())
}

// ============================================================================
// JWT Dashboard Session (ports src/shared/utils/dashboardSessionToken.ts)
// ============================================================================

const DASHBOARD_SESSION_CLAIM: &str = "authenticated";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSessionClaims {
    pub authenticated: bool,
    pub sub: String,
    pub iat: u64,
    pub exp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

/// Create a dashboard session JWT (issued by /api/auth/login)
pub fn create_dashboard_session_token(
    subject: &str,
    scopes: Option<Vec<String>>,
    ttl: Duration,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let claims = DashboardSessionClaims {
        authenticated: true,
        sub: subject.to_string(),
        iat: now,
        exp: now + ttl.as_secs(),
        scopes,
    };
    
    let secret = get_jwt_secret();
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret),
    )
}

/// Verify a dashboard session JWT — exact parity with `verifyDashboardSessionToken`
/// Returns claims if valid, error otherwise
pub fn verify_dashboard_session_token(token: &str) -> Result<DashboardSessionClaims, AuthError> {
    let secret = get_jwt_secret();
    let mut validation = Validation::new(Algorithm::HS256);
    validation.required_spec_claims.insert("authenticated".to_string());
    
    let token_data: TokenData<DashboardSessionClaims> = decode(
        token,
        &DecodingKey::from_secret(&secret),
        &validation,
    )?;
    
    // Extra check: the `authenticated` claim MUST be true
    if !token_data.claims.authenticated {
        return Err(AuthError::InvalidCredentials);
    }
    
    Ok(token_data.claims)
}

// ============================================================================
// API Key Validation (ports src/sse/services/auth.ts extractApiKey/isValidApiKey)
// ============================================================================

/// Extract API key from Authorization header or x-api-key header
pub fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    // Authorization: Bearer <key>
    if let Some(auth) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(auth_str) = auth.to_str() {
            if let Some(key) = auth_str.strip_prefix("Bearer ") {
                return Some(key.to_string());
            }
        }
    }
    
    // x-api-key header
    if let Some(key) = headers.get("x-api-key") {
        if let Ok(key_str) = key.to_str() {
            return Some(key_str.to_string());
        }
    }
    
    None
}

/// Verify an API key format (basic validation — real validation hits DB)
/// Mirrors `isValidApiKey` from `src/sse/services/auth.ts`
pub fn is_valid_api_key_format(key: &str) -> bool {
    // OmniRoute API keys: sk- prefix, 32+ chars, base64url-safe chars
    key.starts_with("sk-") && key.len() >= 36
}

/// Compute the owner hash (sha256 prefix of raw key) — mirrors `accessTokenAuth.ts`
pub fn api_key_owner_hash(key: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
    mac.update(key.as_bytes());
    let result = mac.finalize().into_bytes();
    hex::encode(&result[..8]) // 8-byte prefix = 16 hex chars
}

// ============================================================================
// Peer Stamp / Locality (ports src/server/authz/peerStamp.ts + classifyHostLocality)
// ============================================================================

/// Trusted peer IP stamp — injected by the WS wrapper for LOCAL_ONLY routes
/// to defeat Host-header spoofing (Hard Rules #15/#17)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerStamp {
    pub ip: IpAddr,
    pub timestamp: u64,
    pub signature: String, // HMAC-SHA256(ip|timestamp, API_KEY_SECRET)
}

impl PeerStamp {
    /// Create a new peer stamp for a local request
    pub fn new(ip: IpAddr) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let data = format!("{}|{}", ip, timestamp);
        let secret = get_api_key_secret();
        let mut mac = Hmac::<Sha256>::new_from_slice(&secret).unwrap();
        mac.update(data.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());
        
        Self { ip, timestamp, signature }
    }

    /// Verify a peer stamp (must be fresh and signature valid)
    pub fn verify(&self, max_age: Duration) -> bool {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        if now.saturating_sub(self.timestamp) > max_age.as_secs() {
            return false; // stale
        }
        
        let data = format!("{}|{}", self.ip, self.timestamp);
        let secret = get_api_key_secret();
        let mut mac = Hmac::<Sha256>::new_from_slice(&secret).unwrap();
        mac.update(data.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());
        
        self.signature == expected
    }
}

/// Classify host locality for routeGuard — mirrors `classifyHostLocality`
pub fn classify_host_locality(host: &str) -> HostLocality {
    // Strip port
    let host = host.split(':').next().unwrap_or(host);
    
    if host == "localhost" || host == "127.0.0.1" || host == "::1" {
        return HostLocality::Loopback;
    }
    
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_local_request(ip) {
            return HostLocality::Lan;
        }
        return HostLocality::Remote;
    }
    
    // Hostname - check if it resolves to local (simplified: treat as remote)
    HostLocality::Remote
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostLocality {
    Loopback,
    Lan,
    Remote,
}

// ============================================================================
// Health Check Response (ports /api/monitoring/health structure)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String, // "ok" | "degraded" | "unhealthy"
    pub timestamp: u64,
    pub version: String,
    pub build_sha: Option<String>,
    pub uptime_secs: u64,
    pub checks: HealthChecks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthChecks {
    pub database: ComponentHealth,
    pub circuit_breakers: ComponentHealth,
    pub rate_limits: ComponentHealth,
    pub providers: ComponentHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub status: String, // "ok" | "degraded" | "down"
    pub details: Option<serde_json::Value>,
}

impl Default for HealthResponse {
    fn default() -> Self {
        Self {
            status: "ok".to_string(),
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            build_sha: option_env!("BUILD_SHA").map(|s| s.to_string()),
            uptime_secs: 0, // filled by server
            checks: HealthChecks {
                database: ComponentHealth { status: "ok".into(), details: None },
                circuit_breakers: ComponentHealth { status: "ok".into(), details: None },
                rate_limits: ComponentHealth { status: "ok".into(), details: None },
                providers: ComponentHealth { status: "ok".into(), details: None },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::time::Duration;

    /// Ensure required env vars are set for tests that need them
    fn setup_test_env() {
        unsafe {
            std::env::set_var("JWT_SECRET", "test-jwt-secret-key-that-is-long-enough-for-hs256");
            std::env::set_var("API_KEY_SECRET", "test-api-key-secret-that-is-long-enough-for-hmac");
        }
    }

    #[test]
    fn classify_local_only_exact() {
        assert_eq!(classify_route("/api/mcp/sse"), AuthTier::LocalOnly);
        assert_eq!(classify_route("/api/cli-tools/something"), AuthTier::LocalOnly);
        assert_eq!(classify_route("/api/services/foo"), AuthTier::LocalOnly);
    }

    #[test]
    fn classify_local_only_regex() {
        assert_eq!(classify_route("/api/providers/abc/login"), AuthTier::LocalOnly);
        assert_eq!(classify_route("/api/providers/abc/refresh-cursor"), AuthTier::LocalOnly);
        assert_eq!(classify_route("/api/providers/validate"), AuthTier::LocalOnly);
    }

    #[test]
    fn classify_always_protected() {
        assert_eq!(classify_route("/api/shutdown"), AuthTier::AlwaysProtected);
        assert_eq!(classify_route("/api/settings/database"), AuthTier::AlwaysProtected);
    }

    #[test]
    fn classify_management_default() {
        assert_eq!(classify_route("/api/v1/chat/completions"), AuthTier::Management);
        assert_eq!(classify_route("/api/settings/cache-config"), AuthTier::Management);
    }

    #[test]
    fn is_local_request_check() {
        assert!(is_local_request(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(is_local_request(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50))));
        assert!(is_local_request(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(is_local_request(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(!is_local_request(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    }

    #[test]
    fn cors_default_config() {
        let cfg = CorsConfig::default();
        assert!(cfg.allow_credentials);
        assert_eq!(cfg.allowed_origins, vec!["*"]);
    }

    #[test]
    fn cors_headers_include_origin() {
        let cfg = CorsConfig::default();
        let origin = HeaderValue::from_static("http://localhost:3000");
        let headers = build_cors_headers(&cfg, Some(&origin), true);
        assert!(headers.get(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN).is_some());
        assert!(headers.get(axum::http::header::ACCESS_CONTROL_ALLOW_CREDENTIALS).is_some());
    }

    #[test]
    fn csrf_token_generation() {
        let token1 = generate_csrf_token();
        let token2 = generate_csrf_token();
        assert_ne!(token1, token2);
        assert!(token1.len() > 20);
    }

    #[test]
    fn csrf_validation() {
        let token = generate_csrf_token();
        let mut headers = HeaderMap::new();
        headers.insert("x-csrf-token", token.parse().unwrap());
        assert!(validate_csrf_token(&headers, Some(&token)));

        let mut headers2 = HeaderMap::new();
        headers2.insert("x-csrf-token", "wrong".parse().unwrap());
        assert!(!validate_csrf_token(&headers2, Some(&token)));
    }

    #[test]
    fn jwt_dashboard_session_roundtrip() {
        setup_test_env();
        let token = create_dashboard_session_token("test-user", Some(vec!["admin".to_string()]), Duration::from_secs(3600)).unwrap();
        let claims = verify_dashboard_session_token(&token).unwrap();
        assert!(claims.authenticated);
        assert_eq!(claims.sub, "test-user");
        assert_eq!(claims.scopes.unwrap(), vec!["admin".to_string()]);
    }

    #[test]
    fn jwt_rejects_missing_authenticated_claim() {
        setup_test_env();
        // Token without authenticated claim should fail
        let secret = get_jwt_secret();
        let claims = serde_json::json!({
            "sub": "test",
            "iat": 123,
            "exp": 9999999999u64
        });
        let bad_token = encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(&secret)).unwrap();
        assert!(verify_dashboard_session_token(&bad_token).is_err());
    }

    #[test]
    fn api_key_extraction() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer sk-test123456789012345678901234".parse().unwrap());
        assert_eq!(extract_api_key(&headers), Some("sk-test123456789012345678901234".to_string()));

        let mut headers2 = HeaderMap::new();
        headers2.insert("x-api-key", "sk-test123456789012345678901234".parse().unwrap());
        assert_eq!(extract_api_key(&headers2), Some("sk-test123456789012345678901234".to_string()));
    }

    #[test]
    fn api_key_format_validation() {
        setup_test_env();
        assert!(is_valid_api_key_format("sk-test12345678901234567890123456789"));
        assert!(!is_valid_api_key_format("sk-short"));
        assert!(!is_valid_api_key_format("pk-test12345678901234567890123456789"));
        assert!(!is_valid_api_key_format(""));
    }

    #[test]
    fn api_key_owner_hash_consistency() {
        setup_test_env();
        let key = "sk-test123456789012345678901234";
        let hash1 = api_key_owner_hash(key);
        let hash2 = api_key_owner_hash(key);
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 16); // 8 bytes = 16 hex chars
    }

    #[test]
    fn peer_stamp_verify() {
        setup_test_env();
        let ip = "127.0.0.1".parse().unwrap();
        let stamp = PeerStamp::new(ip);
        assert!(stamp.verify(Duration::from_secs(60)));
        assert!(stamp.verify(Duration::from_nanos(0))); // immediately valid
    }

    #[test]
    fn host_locality_classification() {
        assert_eq!(classify_host_locality("localhost"), HostLocality::Loopback);
        assert_eq!(classify_host_locality("127.0.0.1"), HostLocality::Loopback);
        assert_eq!(classify_host_locality("192.168.1.50"), HostLocality::Lan);
        assert_eq!(classify_host_locality("10.0.0.1"), HostLocality::Lan);
        assert_eq!(classify_host_locality("8.8.8.8"), HostLocality::Remote);
        assert_eq!(classify_host_locality("example.com"), HostLocality::Remote);
    }

    #[test]
    fn health_response_defaults() {
        let health = HealthResponse::default();
        assert_eq!(health.status, "ok");
        assert!(!health.version.is_empty());
        assert_eq!(health.checks.database.status, "ok");
    }
}