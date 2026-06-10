//! Configuration loader for `wm-reach`.
//!
//! Config is read from `/etc/wintermute/conf.d/` (or `WM_REACH_CONF_DIR`).
//! No secrets are ever hard-coded; credentials are read from files at load time
//! and held in memory as opaque strings that are never logged.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Daily digest configuration.
///
/// Controls whether a daily presence summary is delivered, at what local
/// hour, and for which contact name the summary is personalised.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DigestConfig {
    /// Whether to deliver the daily digest at all (opt-in gate).
    #[serde(default)]
    pub enabled: bool,
    /// Local hour (0–23) at which to deliver the digest (default: 20 = 8pm).
    #[serde(default = "default_digest_hour")]
    pub send_hour: u8,
    /// Contact name used in the digest body (e.g. `"Mom"`).
    #[serde(default)]
    pub contact_name: Option<String>,
}

const fn default_digest_hour() -> u8 {
    20
}

/// Silence-nudge configuration.
///
/// A single, debounced, gentle delivery when a waking-hours silence window fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SilenceNudgeConfig {
    /// Whether to deliver a silence nudge at all (opt-in gate; default `false`).
    #[serde(default)]
    pub enabled: bool,
    /// Contact name used in the nudge body (e.g. `"Mom"`).
    #[serde(default = "default_contact_name")]
    pub contact_name: String,
}

fn default_contact_name() -> String {
    "Mom".to_string()
}

impl Default for SilenceNudgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            contact_name: default_contact_name(),
        }
    }
}

/// Inbound transport kind.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InboundTransportKind {
    /// Scan a local maildir `new/` directory (default; no network required).
    #[default]
    Maildir,
    /// IMAP poll (requires the `imap` Cargo feature).
    #[cfg(feature = "imap")]
    Imap,
}

/// Inbound channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundConfig {
    /// Whether the inbound channel is active (opt-in gate; default `false`).
    #[serde(default)]
    pub enabled: bool,
    /// Transport to use for inbound polling.
    #[serde(default)]
    pub transport: InboundTransportKind,
    /// Maildir root path (used when `transport == Maildir`).
    #[serde(default)]
    pub mailbox: Option<String>,
    /// Poll interval in seconds.
    #[serde(default = "default_poll_secs")]
    pub poll_secs: u64,
    /// Allowlist of `From` email addresses.
    #[serde(default)]
    pub allow_from: Vec<String>,
    /// Display name used in published replies (e.g. `"Joe"`).
    #[serde(default = "default_display_name")]
    pub display_name: String,
}

fn default_poll_secs() -> u64 {
    60
}

fn default_display_name() -> String {
    "Joe".to_string()
}

impl Default for InboundConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            transport: InboundTransportKind::default(),
            mailbox: None,
            poll_secs: default_poll_secs(),
            allow_from: Vec::new(),
            display_name: default_display_name(),
        }
    }
}

/// Distress delivery policy — retry + fallback ladder for `wm.family.distress`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistressPolicy {
    /// Maximum retry attempts on the primary transport (0 = no retry).
    #[serde(default = "default_distress_retries")]
    pub max_retries: u32,
    /// Base backoff in milliseconds between retry attempts.
    #[serde(default = "default_distress_backoff_ms")]
    pub backoff_ms: u64,
    /// Ordered list of fallback transport kinds to try after primary retries fail.
    /// Each element is a transport kind name (e.g. `"ntfy"`, `"webhook"`).
    #[serde(default)]
    pub fallback_order: Vec<String>,
}

fn default_distress_retries() -> u32 {
    3
}

fn default_distress_backoff_ms() -> u64 {
    500
}

impl Default for DistressPolicy {
    fn default() -> Self {
        Self {
            max_retries: default_distress_retries(),
            backoff_ms: default_distress_backoff_ms(),
            fallback_order: Vec::new(),
        }
    }
}

/// Top-level daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Transport backend to use.
    pub transport: TransportConfig,
    /// Contact name to use in the `from` field of `wm.family.reply`.
    #[serde(default = "default_from")]
    pub from: String,
    /// Daily digest settings (opt-in; disabled by default).
    #[serde(default)]
    pub digest: DigestConfig,
    /// Silence nudge settings (opt-in; disabled by default).
    #[serde(default)]
    pub silence_nudge: SilenceNudgeConfig,
    /// Inbound channel settings (opt-in; disabled by default).
    #[serde(default)]
    pub inbound: InboundConfig,
    /// Distress delivery policy (retry + fallback ladder).
    #[serde(default)]
    pub distress_policy: DistressPolicy,
}

fn default_from() -> String {
    "wintermute".to_string()
}

/// Transport backend selection and per-backend config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransportConfig {
    /// Email via sendmail or SMTP submission.
    Email(EmailConfig),
    /// ntfy push notification (requires the `ntfy` Cargo feature).
    #[cfg(feature = "ntfy")]
    Ntfy(NtfyConfig),
    /// Generic webhook POST (requires the `webhook` Cargo feature).
    #[cfg(feature = "webhook")]
    Webhook(WebhookConfig),
}

/// Email transport configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    /// Recipient email address.
    pub to: String,
    /// Sender email address.
    pub from: String,
    /// Path to sendmail binary. Defaults to `/usr/sbin/sendmail`.
    #[serde(default = "default_sendmail")]
    pub sendmail: String,
    /// SMTP submission host (optional; used only if sendmail integration is unavailable).
    #[serde(default)]
    pub smtp_host: Option<String>,
}

fn default_sendmail() -> String {
    "/usr/sbin/sendmail".to_string()
}

/// ntfy transport configuration (feature = "ntfy").
#[cfg(feature = "ntfy")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NtfyConfig {
    /// ntfy topic URL (e.g. `https://ntfy.sh/my-topic`).
    pub topic_url: String,
}

/// Webhook transport configuration (feature = "webhook").
#[cfg(feature = "webhook")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Webhook POST URL.
    pub url: String,
}

impl Default for Config {
    /// Default config used as the base for `..Default::default()` in tests.
    fn default() -> Self {
        Self {
            transport: TransportConfig::Email(EmailConfig {
                to: String::new(),
                from: String::new(),
                sendmail: default_sendmail(),
                smtp_host: None,
            }),
            from: default_from(),
            digest: DigestConfig::default(),
            silence_nudge: SilenceNudgeConfig::default(),
            inbound: InboundConfig::default(),
            distress_policy: DistressPolicy::default(),
        }
    }
}

impl Config {
    /// Load config from `conf_dir/reach.json` or `conf_dir/reach.toml`.
    ///
    /// Falls back to a default email config using `WM_REACH_TO` / `WM_REACH_FROM`
    /// environment variables when no config file is present (for test environments).
    ///
    /// # Errors
    ///
    /// Returns `Err` if a config file exists but fails to parse.
    pub fn load(conf_dir: &Path) -> Result<Self> {
        let json_path = conf_dir.join("reach.json");

        if json_path.exists() {
            let raw = std::fs::read_to_string(&json_path)
                .with_context(|| format!("reading {}", json_path.display()))?;
            let cfg: Self = serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", json_path.display()))?;
            return Ok(cfg);
        }

        // No config file — build a minimal default from environment.
        // WM_REACH_SENDMAIL may override the sendmail path (used in tests).
        let sendmail = std::env::var("WM_REACH_SENDMAIL")
            .unwrap_or_else(|_| "/usr/sbin/sendmail".to_string());
        let to = std::env::var("WM_REACH_TO")
            .unwrap_or_else(|_| "jyen.tech@gmail.com".to_string());
        let from_addr = std::env::var("WM_REACH_FROM")
            .unwrap_or_else(|_| "wintermute@localhost".to_string());

        Ok(Self {
            transport: TransportConfig::Email(EmailConfig {
                to,
                from: from_addr,
                sendmail,
                smtp_host: None,
            }),
            from: "wintermute".to_string(),
            digest: DigestConfig::default(),
            silence_nudge: SilenceNudgeConfig::default(),
            inbound: InboundConfig::default(),
            distress_policy: DistressPolicy::default(),
        })
    }

    /// Return the active transport kind as a static string (for ack payloads).
    #[must_use]
    pub const fn transport_kind(&self) -> &'static str {
        match &self.transport {
            TransportConfig::Email(_) => "email",
            #[cfg(feature = "ntfy")]
            TransportConfig::Ntfy(_) => "ntfy",
            #[cfg(feature = "webhook")]
            TransportConfig::Webhook(_) => "webhook",
        }
    }
}
