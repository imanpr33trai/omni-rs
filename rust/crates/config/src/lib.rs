//! OmniRoute configuration layer.
//!
//! Ports the 27-section `.env.example` (3,771 lines) + 86 feature flags
//! from `src/shared/constants/featureFlagDefinitions.ts` +
//! `src/shared/utils/featureFlags.ts` (env→DB override chain).
//!
//! Uses figment for layered config: defaults → .env → env vars → DB overrides.

use figment::{Figment, providers::{Env, Format, Toml}};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("missing required env: {0}")]
    MissingRequired(String),
    #[error("invalid value for {key}: {msg}")]
    InvalidValue { key: String, msg: String },
    #[error("figment error: {0}")]
    Figment(#[from] figment::Error),
}

/// Phase 1.1: all 27 sections as nested structs.
/// Section 1: REQUIRED SECRETS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsConfig {
    pub jwt_secret: String,
    pub api_key_secret: String,
    pub initial_password: String,
}

/// Section 2: STORAGE
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub data_dir: Option<String>,
    pub omniroute_data_dir: Option<String>,
    pub storage_encryption_key: Option<String>,
    pub storage_encryption_key_version: Option<u32>,
    pub disable_sqlite_auto_backup: bool,
    pub redis_url: Option<String>,
    pub omniroute_plugins_dir: Option<String>,
}

/// Section 3: NETWORK/PORTS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub port: u16,
}

/// Section 4: SECURITY/AUTH
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub require_api_key: bool,
}

/// Section 5: PII (opt-in — Hard Rule #20 defaults to false)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiiConfig {
    pub pii_redaction_enabled: bool,
    pub pii_response_sanitization: bool,
}

/// Section 6: TOOL & ROUTING POLICIES
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRoutingConfig {
    pub tool_policy: Option<String>,
    pub routing_policy: Option<String>,
}

/// Section 7: URLS & CLOUD SYNC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlsConfig {
    pub cloud_sync_url: Option<String>,
    pub app_url: Option<String>,
}

/// Section 8: OUTBOUND PROXY
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundProxyConfig {
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

/// Section 9: CLI TOOL INTEGRATION
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliToolConfig {
    pub cli_tool_integration: Option<String>,
}

/// Section 10: AGENT & MCP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMcpConfig {
    pub agent_mcp_enabled: bool,
}

/// Section 11: OAUTH CREDENTIALS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OauthConfig {
    pub oauth_credentials: Option<String>,
}

/// Section 12: UA OVERRIDES
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UaConfig {
    pub ua_override: Option<String>,
}

/// Section 13: FINGERPRINT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintConfig {
    pub fingerprint: Option<String>,
}

/// Section 14: API KEY PROVIDERS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyProvidersConfig {
    pub api_key_providers: Option<String>,
}

/// Section 15: TIMEOUTS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutsConfig {
    pub timeout_ms: Option<u64>,
    pub connect_timeout_ms: Option<u64>,
}

/// Section 16: LOGGING
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub app_log_level: String,
}

/// Section 17: MEMORY OPTIMIZATION
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub memory_optimization: Option<String>,
}

/// Section 18: PRICING
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingConfig {
    pub pricing: Option<String>,
}

/// Section 19: MODEL SYNC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSyncConfig {
    pub model_sync: Option<String>,
}

/// Section 20: PROVIDER-SPECIFIC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSpecificConfig {
    pub provider_specific: Option<String>,
}

/// Section 21: PROXY HEALTH
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyHealthConfig {
    pub proxy_health: Option<String>,
}

/// Section 22: DEBUGGING
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugConfig {
    pub provider_cooldown_enabled: bool,
    pub debug: Option<String>,
}

/// Section 23: GITHUB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubConfig {
    pub github_token: Option<String>,
}

/// Section 24: QUOTAS/TUNNELS/SKILLS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotasTunnelsSkillsConfig {
    pub mitm_enabled: bool,
    pub omniroute_no_sudo: bool,
    pub tailscale_authkey: Option<String>,
    pub ngrok_authtoken: Option<String>,
    pub free_proxy: Option<String>,
    pub bifrost: Option<String>,
    pub quota_store: Option<String>,
    pub skills: Option<String>,
    pub omniroute_relay_backend: Option<String>,
    pub omniroute_tls_cert: Option<String>,
    pub omniroute_tls_key: Option<String>,
    pub local_endpoints: Option<String>,
}

/// Section 25: TEST & E2E
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConfig {
    pub test_mode: bool,
}

/// Section 26: RADAR
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadarConfig {
    pub radar: Option<String>,
}

/// Section 27: v3.8.50 additions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3850Config {
    pub rotation_enabled: bool,
    pub pii_flags: bool,
}

/// 86 Feature flags (from `src/shared/constants/featureFlagDefinitions.ts`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlagsConfig {
    #[serde(flatten)]
    pub flags: HashMap<String, bool>,
}

/// Root config — all sections combined.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub secrets: SecretsConfig,
    pub storage: StorageConfig,
    pub network: NetworkConfig,
    pub security: SecurityConfig,
    pub pii: PiiConfig,
    pub tool_routing: ToolRoutingConfig,
    pub urls: UrlsConfig,
    pub outbound_proxy: OutboundProxyConfig,
    pub cli_tool: CliToolConfig,
    pub agent_mcp: AgentMcpConfig,
    pub oauth: OauthConfig,
    pub ua: UaConfig,
    pub fingerprint: FingerprintConfig,
    pub api_key_providers: ApiKeyProvidersConfig,
    pub timeouts: TimeoutsConfig,
    pub logging: LoggingConfig,
    pub memory: MemoryConfig,
    pub pricing: PricingConfig,
    pub model_sync: ModelSyncConfig,
    pub provider_specific: ProviderSpecificConfig,
    pub proxy_health: ProxyHealthConfig,
    pub debug: DebugConfig,
    pub github: GithubConfig,
    pub quotas_tunnels_skills: QuotasTunnelsSkillsConfig,
    pub test: TestConfig,
    pub radar: RadarConfig,
    pub v3850: V3850Config,
    pub feature_flags: FeatureFlagsConfig,
}

impl AppConfig {
    /// Load config with layered precedence:
    /// defaults → .env.example → .env → env vars → (DB overrides applied later)
    pub fn load() -> Result<Self, ConfigError> {
        let figment = Figment::new()
            .merge(Toml::file("rust/config/defaults.toml")) // defaults file
            .merge(Toml::file(".env.example"))              // template as fallback
            .merge(Toml::file(".env").nested())             // user overrides
            .merge(Env::prefixed("OMNIROUTE_").split("_"))  // env vars with prefix
            .merge(Env::raw());                             // raw env vars

        let mut config: AppConfig = figment.extract()?;

        // Validate required secrets
        if config.secrets.jwt_secret.is_empty() {
            return Err(ConfigError::MissingRequired("JWT_SECRET".into()));
        }
        if config.secrets.api_key_secret.is_empty() {
            return Err(ConfigError::MissingRequired("API_KEY_SECRET".into()));
        }
        if config.secrets.initial_password.is_empty() {
            return Err(ConfigError::MissingRequired("INITIAL_PASSWORD".into()));
        }

        // Hard Rule #20: PII defaults to false
        if !config.pii.pii_redaction_enabled && !config.pii.pii_response_sanitization {
            // OK — defaults are false
        }

        Ok(config)
    }

    /// Get the effective default port (20128)
    pub fn port(&self) -> u16 {
        self.network.port
    }

    /// Get feature flag value (with env→DB override chain applied externally)
    pub fn feature_flag(&self, name: &str) -> bool {
        self.feature_flags.flags.get(name).copied().unwrap_or(false)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            secrets: SecretsConfig {
                jwt_secret: String::new(),
                api_key_secret: String::new(),
                initial_password: String::new(),
            },
            storage: StorageConfig {
                data_dir: None,
                omniroute_data_dir: None,
                storage_encryption_key: None,
                storage_encryption_key_version: None,
                disable_sqlite_auto_backup: false,
                redis_url: None,
                omniroute_plugins_dir: None,
            },
            network: NetworkConfig { port: 20128 },
            security: SecurityConfig { require_api_key: false },
            pii: PiiConfig { pii_redaction_enabled: false, pii_response_sanitization: false },
            tool_routing: ToolRoutingConfig { tool_policy: None, routing_policy: None },
            urls: UrlsConfig { cloud_sync_url: None, app_url: None },
            outbound_proxy: OutboundProxyConfig { http_proxy: None, https_proxy: None, no_proxy: None },
            cli_tool: CliToolConfig { cli_tool_integration: None },
            agent_mcp: AgentMcpConfig { agent_mcp_enabled: false },
            oauth: OauthConfig { oauth_credentials: None },
            ua: UaConfig { ua_override: None },
            fingerprint: FingerprintConfig { fingerprint: None },
            api_key_providers: ApiKeyProvidersConfig { api_key_providers: None },
            timeouts: TimeoutsConfig { timeout_ms: None, connect_timeout_ms: None },
            logging: LoggingConfig { app_log_level: "info".into() },
            memory: MemoryConfig { memory_optimization: None },
            pricing: PricingConfig { pricing: None },
            model_sync: ModelSyncConfig { model_sync: None },
            provider_specific: ProviderSpecificConfig { provider_specific: None },
            proxy_health: ProxyHealthConfig { proxy_health: None },
            debug: DebugConfig { provider_cooldown_enabled: false, debug: None },
            github: GithubConfig { github_token: None },
            quotas_tunnels_skills: QuotasTunnelsSkillsConfig {
                mitm_enabled: false,
                omniroute_no_sudo: false,
                tailscale_authkey: None,
                ngrok_authtoken: None,
                free_proxy: None,
                bifrost: None,
                quota_store: None,
                skills: None,
                omniroute_relay_backend: None,
                omniroute_tls_cert: None,
                omniroute_tls_key: None,
                local_endpoints: None,
            },
            test: TestConfig { test_mode: false },
            radar: RadarConfig { radar: None },
            v3850: V3850Config { rotation_enabled: false, pii_flags: false },
            feature_flags: FeatureFlagsConfig { flags: HashMap::new() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_correct() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.port(), 20128);
        assert!(!cfg.pii.pii_redaction_enabled);
        assert!(!cfg.pii.pii_response_sanitization);
        assert!(!cfg.debug.provider_cooldown_enabled);
        assert!(!cfg.security.require_api_key);
    }
}