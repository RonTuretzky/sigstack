//! Application configuration loaded from environment variables.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

/// Application configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Signal configuration
    #[serde(default)]
    pub signal: SignalConfig,

    /// NEAR AI configuration
    pub near_ai: NearAiConfig,

    /// Conversation storage configuration
    #[serde(default)]
    pub conversation: ConversationConfig,

    /// Bot configuration
    #[serde(default)]
    pub bot: BotConfig,

    /// Dstack configuration
    #[serde(default)]
    pub dstack: DstackConfig,

    /// Tools configuration
    #[serde(default)]
    pub tools: ToolsConfig,

    /// Translation configuration
    #[serde(default)]
    pub translation: TranslationConfig,

    /// Payment configuration
    #[serde(default)]
    pub payments: x402_payments::PaymentConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SignalConfig {
    /// Signal CLI REST API endpoint
    #[serde(default = "default_signal_service")]
    pub service_url: String,

    /// Poll interval for messages (only used in polling mode)
    #[serde(default = "default_poll_interval", with = "humantime_serde")]
    pub poll_interval: Duration,

    /// Use WebSocket for receiving messages (requires Signal CLI json-rpc mode)
    #[serde(default)]
    pub use_websocket: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NearAiConfig {
    /// NEAR AI API key
    pub api_key: String,

    /// API base URL
    #[serde(default = "default_near_ai_url")]
    pub base_url: String,

    /// Default model
    #[serde(default = "default_model")]
    pub model: String,

    /// Request timeout
    #[serde(default = "default_timeout", with = "humantime_serde")]
    pub timeout: Duration,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConversationConfig {
    /// Conversation TTL (how long before inactive conversations expire)
    #[serde(default = "default_ttl", with = "humantime_serde")]
    pub ttl: Duration,

    /// Max messages per conversation (older messages are trimmed)
    #[serde(default = "default_max_messages")]
    pub max_messages: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BotConfig {
    /// System prompt for AI
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,

    /// Signal username (e.g., "nearai.54")
    #[serde(default)]
    pub signal_username: Option<String>,

    /// GitHub repository URL
    #[serde(default)]
    pub github_repo: Option<String>,

    /// Log level
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DstackConfig {
    /// Dstack guest agent socket path
    #[serde(default = "default_dstack_socket")]
    pub socket_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolsConfig {
    /// Enable tool use system
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum tool calls per message
    #[serde(default = "default_max_tool_calls")]
    pub max_tool_calls: usize,

    /// Web search configuration
    #[serde(default)]
    pub web_search: WebSearchConfig,

    /// Weather tool configuration
    #[serde(default)]
    pub weather: WeatherConfig,

    /// Calculator tool configuration
    #[serde(default)]
    pub calculator: CalculatorConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebSearchConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub api_key: Option<String>,
    #[serde(default = "default_search_results")]
    pub max_results: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WeatherConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CalculatorConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranslationConfig {
    /// Enable automatic translation
    #[serde(default)]
    pub enabled: bool,

    /// LibreTranslate API URL
    #[serde(default = "default_libretranslate_url")]
    pub libretranslate_url: String,

    /// Optional LibreTranslate API key
    #[serde(default)]
    pub libretranslate_api_key: Option<String>,

    /// Group-to-language-pair mappings.
    /// Format: "group_id1:en:es,group_id2:en:fr"
    #[serde(default)]
    pub groups: String,

    /// Whether translated groups should also get AI chat responses
    #[serde(default)]
    pub also_chat: bool,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            libretranslate_url: default_libretranslate_url(),
            libretranslate_api_key: None,
            groups: String::new(),
            also_chat: false,
        }
    }
}

fn default_libretranslate_url() -> String {
    "http://localhost:5000".into()
}

// Default implementations
impl Default for SignalConfig {
    fn default() -> Self {
        Self {
            service_url: default_signal_service(),
            poll_interval: default_poll_interval(),
            use_websocket: false,
        }
    }
}

impl Default for ConversationConfig {
    fn default() -> Self {
        Self {
            ttl: default_ttl(),
            max_messages: default_max_messages(),
        }
    }
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            system_prompt: default_system_prompt(),
            signal_username: None,
            github_repo: None,
            log_level: default_log_level(),
        }
    }
}

impl Default for DstackConfig {
    fn default() -> Self {
        Self {
            socket_path: default_dstack_socket(),
        }
    }
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            max_tool_calls: default_max_tool_calls(),
            web_search: WebSearchConfig::default(),
            weather: WeatherConfig::default(),
            calculator: CalculatorConfig::default(),
        }
    }
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            api_key: None,
            max_results: default_search_results(),
        }
    }
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
        }
    }
}

impl Default for CalculatorConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
        }
    }
}

// Default value functions
fn default_signal_service() -> String {
    "http://signal-api:8080".into()
}

fn default_poll_interval() -> Duration {
    Duration::from_secs(5)
}

fn default_near_ai_url() -> String {
    "https://cloud-api.near.ai/v1".into()
}

fn default_model() -> String {
    "deepseek-ai/DeepSeek-V3.1".into()
}

fn default_timeout() -> Duration {
    Duration::from_secs(10)
}

fn default_ttl() -> Duration {
    Duration::from_secs(24 * 60 * 60) // 24 hours
}

fn default_max_messages() -> usize {
    50
}

fn default_system_prompt() -> String {
    r#"You are an AI assistant accessible via Signal, running in a Trusted Execution Environment (TEE) for privacy protection.

## Privacy & Security
- Your conversations are protected by Intel TDX hardware encryption
- Neither the bot operator nor the AI provider can read your messages
- Users can verify this by sending "!verify" for cryptographic attestation

## Available Tools
You have access to these tools - use them when helpful:
- **web_search**: Search the web for current information, news, facts
- **get_weather**: Get current weather for any location
- **calculate**: Evaluate math expressions accurately

## Guidelines
- Be concise - this is mobile chat, not essays
- Use tools proactively for current information (don't guess dates, prices, weather)
- For calculations, use the calculate tool rather than mental math
- If a tool fails, explain what happened and try to help anyway
- Never fabricate search results or weather data"#.into()
}

/// Build system prompt with identity information.
/// This is called at runtime to inject signal_username and github_repo.
pub fn build_system_prompt_with_identity(
    base_prompt: &str,
    signal_username: Option<&str>,
    github_repo: Option<&str>,
) -> String {
    let now = chrono::Utc::now();
    let mut prompt = base_prompt.to_string();

    // Add identity section if either field is configured
    if signal_username.is_some() || github_repo.is_some() {
        prompt.push_str("\n\n## Identity");
        if let Some(username) = signal_username {
            prompt.push_str(&format!("\n- Signal username: @{}", username));
        }
        if let Some(repo) = github_repo {
            prompt.push_str(&format!("\n- Source code: {}", repo));
        }
    }

    // Add current timestamp
    prompt.push_str(&format!(
        "\n\nCurrent date and time: {} UTC",
        now.format("%A, %B %d, %Y at %H:%M")
    ));

    prompt
}

fn default_log_level() -> String {
    "info".into()
}

fn default_dstack_socket() -> String {
    "/var/run/dstack.sock".into()
}

fn default_true() -> bool {
    true
}

fn default_max_tool_calls() -> usize {
    5
}

fn default_search_results() -> usize {
    5
}

impl Config {
    /// Load configuration from environment variables.
    pub fn load() -> Result<Self> {
        // Load .env file if present
        dotenvy::dotenv().ok();

        let config = config::Config::builder()
            .add_source(
                config::Environment::default()
                    .separator("__")
                    // Note: try_parsing(true) would parse +16504928286 as a positive number
                    // stripping the + prefix. Keep strings as strings.
                    .try_parsing(false),
            )
            .build()
            .context("Failed to build configuration")?;

        config
            .try_deserialize()
            .context("Failed to deserialize configuration")
    }
}
