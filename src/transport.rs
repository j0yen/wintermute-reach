//! Transport trait and backend implementations.
//!
//! The `Transport` trait is the single abstraction point for off-device delivery.
//! Each backend returns a `DeliveryResult` that maps to `wm.family.ack` payload.
//!
//! Credentials are never logged. The `Debug` impl on config types must not
//! expose secrets (ensured by the custom Debug impls below).

// Items are pub for the lib crate (used by integration tests); the bin
// compiles the same modules in-tree.
#![allow(unreachable_pub)]

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::{Config, TransportConfig};

/// Outcome of a single delivery attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryResult {
    /// Whether delivery succeeded.
    pub delivered: bool,
    /// Active transport kind (e.g. `"email"`).
    pub transport: String,
    /// Opaque delivery reference (e.g. message-id for email).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Error description when `delivered == false`. Never contains credentials.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Core transport abstraction.
pub trait Transport {
    /// Attempt delivery of `body` to the configured recipient.
    ///
    /// Returns `DeliveryResult` regardless of success/failure.
    /// Must never panic; errors are encoded in `DeliveryResult::error`.
    ///
    /// # Errors
    ///
    /// This method itself should not return `Err`; transport-level failures
    /// are encoded as `DeliveryResult { delivered: false, error: Some(...) }`.
    fn deliver(&self, subject: &str, body: &str) -> Result<DeliveryResult>;
}

/// Email transport — invokes sendmail.
pub struct EmailTransport {
    to: String,
    from: String,
    sendmail: String,
}

impl EmailTransport {
    /// Construct from config.
    #[must_use]
    pub(crate) const fn new(to: String, from: String, sendmail: String) -> Self {
        Self { to, from, sendmail }
    }
}

impl Transport for EmailTransport {
    fn deliver(&self, subject: &str, body: &str) -> Result<DeliveryResult> {
        use std::io::Write as _;
        use std::process::{Command, Stdio};

        let message = format!(
            "From: {}\r\nTo: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{}\r\n",
            self.from, self.to, subject, body
        );

        let mut child = match Command::new(&self.sendmail)
            .arg("-oi")
            .arg("-t")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                return Ok(DeliveryResult {
                    delivered: false,
                    transport: "email".to_string(),
                    reference: None,
                    error: Some(format!("sendmail spawn failed: {e}")),
                });
            }
        };

        if let Some(ref mut stdin) = child.stdin.take() {
            match stdin.write_all(message.as_bytes()) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                    // sendmail exited before reading all input — normal for stubs
                    // like `/bin/true`; let wait_with_output() determine success.
                }
                Err(e) => {
                    return Ok(DeliveryResult {
                        delivered: false,
                        transport: "email".to_string(),
                        reference: None,
                        error: Some(format!("sendmail write failed: {e}")),
                    });
                }
            }
        }

        let output = match child.wait_with_output() {
            Ok(o) => o,
            Err(e) => {
                return Ok(DeliveryResult {
                    delivered: false,
                    transport: "email".to_string(),
                    reference: None,
                    error: Some(format!("sendmail wait failed: {e}")),
                });
            }
        };

        if output.status.success() {
            Ok(DeliveryResult {
                delivered: true,
                transport: "email".to_string(),
                reference: None,
                error: None,
            })
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Truncate to avoid leaking large error blobs; never log credentials.
            let err_msg = stderr.chars().take(200).collect::<String>();
            Ok(DeliveryResult {
                delivered: false,
                transport: "email".to_string(),
                reference: None,
                error: Some(format!(
                    "sendmail exited {}: {err_msg}",
                    output.status.code().unwrap_or(-1)
                )),
            })
        }
    }
}

/// ntfy transport (feature = "ntfy").
#[cfg(feature = "ntfy")]
pub struct NtfyTransport {
    topic_url: String,
}

#[cfg(feature = "ntfy")]
impl NtfyTransport {
    /// Construct from config.
    #[must_use]
    pub fn new(topic_url: String) -> Self {
        Self { topic_url }
    }
}

#[cfg(feature = "ntfy")]
impl Transport for NtfyTransport {
    fn deliver(&self, subject: &str, body: &str) -> Result<DeliveryResult> {
        // v1 stub: ntfy POST via std::process (avoids reqwest dep).
        let payload = serde_json::json!({ "topic": &self.topic_url, "title": subject, "message": body });
        let _ = payload; // suppress unused warning in stub
        // Real implementation would POST to self.topic_url.
        // For now, return a stub result (ntfy feature not yet integrated).
        Ok(DeliveryResult {
            delivered: false,
            transport: "ntfy".to_string(),
            reference: None,
            error: Some("ntfy transport: v1 stub not yet implemented".to_string()),
        })
    }
}

/// Webhook transport (feature = "webhook").
#[cfg(feature = "webhook")]
pub struct WebhookTransport {
    url: String,
}

#[cfg(feature = "webhook")]
impl WebhookTransport {
    /// Construct from config.
    #[must_use]
    pub fn new(url: String) -> Self {
        Self { url }
    }
}

#[cfg(feature = "webhook")]
impl Transport for WebhookTransport {
    fn deliver(&self, subject: &str, body: &str) -> Result<DeliveryResult> {
        let _ = (&self.url, subject, body);
        Ok(DeliveryResult {
            delivered: false,
            transport: "webhook".to_string(),
            reference: None,
            error: Some("webhook transport: v1 stub not yet implemented".to_string()),
        })
    }
}

/// Construct the active transport from config and run a test delivery.
///
/// # Errors
///
/// Returns `Err` if the config refers to a feature that is not compiled in.
#[allow(clippy::unnecessary_wraps)] // always Ok in default build
pub fn test_transport(cfg: &Config) -> Result<DeliveryResult> {
    let transport = build_transport(cfg)?;
    transport.deliver("[wm-reach test]", "This is a test delivery from wm-reach test-transport.")
}

/// Build the concrete transport object from config.
///
/// # Errors
///
/// Returns `Err` if the config kind requires a Cargo feature not compiled in.
#[allow(clippy::unnecessary_wraps)] // always Ok in default build; Err path present for feature builds
pub fn build_transport(cfg: &Config) -> Result<Box<dyn Transport>> {
    match &cfg.transport {
        TransportConfig::Email(ec) => Ok(Box::new(EmailTransport::new(
            ec.to.clone(),
            ec.from.clone(),
            ec.sendmail.clone(),
        ))),
        #[cfg(feature = "ntfy")]
        TransportConfig::Ntfy(nc) => Ok(Box::new(NtfyTransport::new(nc.topic_url.clone()))),
        #[cfg(feature = "webhook")]
        TransportConfig::Webhook(wc) => Ok(Box::new(WebhookTransport::new(wc.url.clone()))),
    }
}
