//! Inbound channel — polls a maildir for caregiver replies and publishes
//! `wm.family.reply` events that the dialog's `FamilyFsm::on_reply` speaks aloud.
//!
//! # Design invariants
//!
//! - **Default OFF** — `InboundConfig::enabled` is `false`.  Existing deployments
//!   are unchanged until explicitly enrolled.
//! - **From-allowlist gate** — only messages whose `From` address appears in
//!   `allow_from` are published.  Unauthorised senders are counted and logged at
//!   info level (no body is logged at any level).
//! - **De-dup** — consumed maildir messages are moved to `cur/`; a second poll
//!   tick never re-emits them.
//! - **IMAP behind feature flag** — the `imap` Cargo feature adds `ImapInbound`;
//!   the default build has no `async-imap` dependency.

#![allow(clippy::module_name_repetitions, clippy::print_stderr)]

use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};

// --------------------------------------------------------------------------
// Reply type
// --------------------------------------------------------------------------

/// A parsed inbound reply ready for publishing as `wm.family.reply`.
#[derive(Debug, Clone)]
pub struct InboundReply {
    /// Display name of the sender (from `display_name` config, not raw email).
    pub from: String,
    /// Plain-text body of the reply.
    pub body: String,
    /// Unix timestamp when the message was received.
    pub ts: u64,
}

// --------------------------------------------------------------------------
// Poll counters
// --------------------------------------------------------------------------

/// Counters returned from a single poll tick.
#[derive(Debug, Clone, Default)]
pub struct PollResult {
    /// Number of replies published.
    pub published: usize,
    /// Number of messages dropped due to unauthorized `From`.
    pub dropped_unauthorized: usize,
}

// --------------------------------------------------------------------------
// Maildir inbound
// --------------------------------------------------------------------------

/// Inbound backend that reads from a local Maildir `new/` directory.
///
/// Each message in `new/` that passes the allowlist gate is parsed and
/// returned; the file is then moved to `cur/` so the next poll tick skips it.
pub struct MaildirInbound {
    maildir_root: PathBuf,
    allow_from: Vec<String>,
    display_name: String,
}

impl MaildirInbound {
    /// Construct from a maildir root path and the caregiver allowlist.
    #[must_use]
    pub fn new(maildir_root: PathBuf, allow_from: Vec<String>, display_name: String) -> Self {
        Self {
            maildir_root,
            allow_from,
            display_name,
        }
    }

    /// Poll `new/` for messages, apply the allowlist gate, and return accepted replies.
    ///
    /// Consumed messages are moved to `cur/`; on move failure the message is skipped
    /// (conservative: prefer not double-speaking over dropping a real reply).
    ///
    /// # Errors
    ///
    /// Returns `Err` if `new/` cannot be read.  Per-file errors are logged and skipped.
    pub fn poll(&self) -> Result<(Vec<InboundReply>, PollResult)> {
        let new_dir = self.maildir_root.join("new");
        let cur_dir = self.maildir_root.join("cur");

        // Ensure cur/ exists.
        std::fs::create_dir_all(&cur_dir)
            .with_context(|| format!("creating maildir cur dir {}", cur_dir.display()))?;

        let entries = std::fs::read_dir(&new_dir)
            .with_context(|| format!("reading maildir new dir {}", new_dir.display()))?;

        let mut replies = Vec::new();
        let mut result = PollResult::default();

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let raw = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            match parse_mail_message(&raw) {
                Some((from_addr, body)) => {
                    // Allowlist gate.
                    if !self.is_allowed(&from_addr) {
                        eprintln!(
                            "{{\"level\":\"info\",\"action\":\"inbound_dropped\",\"reason\":\"unauthorized\",\"from\":\"{}\"}}",
                            sanitize_for_json(&from_addr)
                        );
                        result.dropped_unauthorized += 1;
                        // Move to cur/ so it doesn't clutter future polls.
                        let _ = move_to_cur(&path, &cur_dir);
                        continue;
                    }

                    // Move to cur/ before publishing (de-dup guard).
                    if move_to_cur(&path, &cur_dir).is_err() {
                        // Can't move — skip to avoid double-speak.
                        continue;
                    }

                    replies.push(InboundReply {
                        from: self.display_name.clone(),
                        body,
                        ts: unix_now_secs(),
                    });
                    result.published += 1;
                }
                None => {
                    // Unparseable — move to cur/ so it doesn't loop forever.
                    let _ = move_to_cur(&path, &cur_dir);
                }
            }
        }

        Ok((replies, result))
    }

    /// Check whether `from_addr` is in the allow list (case-insensitive).
    fn is_allowed(&self, from_addr: &str) -> bool {
        let lower = from_addr.to_lowercase();
        self.allow_from
            .iter()
            .any(|a| a.to_lowercase() == lower)
    }
}

/// Parse a raw RFC 2822 message string into `(from_address, body)`.
///
/// Handles the most common formats: bare address and `Display Name <addr>`.
/// Returns `None` if the `From` header is missing or the body is empty.
fn parse_mail_message(raw: &str) -> Option<(String, String)> {
    // Split headers from body at first blank line.
    let (headers_raw, body_raw) = raw.split_once("\n\n").or_else(|| raw.split_once("\r\n\r\n"))?;

    let from_addr = extract_from_header(headers_raw)?;
    let body = body_raw.trim().to_string();
    if body.is_empty() {
        return None;
    }

    Some((from_addr, body))
}

/// Extract and normalise the email address from a `From:` header.
fn extract_from_header(headers: &str) -> Option<String> {
    for line in headers.lines() {
        // Header folding: continuation lines start with whitespace.
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let lower = line.to_lowercase();
        if lower.starts_with("from:") {
            let value = line["from:".len()..].trim();
            return Some(parse_address(value));
        }
    }
    None
}

/// Extract the bare email address from `Display Name <addr>` or `addr`.
fn parse_address(value: &str) -> String {
    if let Some(start) = value.find('<') {
        if let Some(end) = value[start..].find('>') {
            return value[start + 1..start + end].trim().to_string();
        }
    }
    // No angle brackets — treat the whole value as the address.
    value.trim().to_string()
}

/// Move a file from `new/` to `cur/` (maildir de-dup guard).
fn move_to_cur(src: &Path, cur_dir: &Path) -> Result<()> {
    let file_name = src
        .file_name()
        .context("missing filename")?;
    let dst = cur_dir.join(file_name);
    std::fs::rename(src, &dst)
        .with_context(|| format!("moving {} to {}", src.display(), dst.display()))
}

/// Sanitise a string for embedding in a JSON log value (escape quotes).
fn sanitize_for_json(s: &str) -> String {
    s.replace('"', "'")
}

/// Current UNIX seconds.
fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// --------------------------------------------------------------------------
// IMAP inbound (feature-gated)
// --------------------------------------------------------------------------

/// IMAP inbound backend (requires the `imap` Cargo feature).
///
/// Fetches UNSEEN messages from the configured folder and marks them
/// `\Seen` after publish so a second poll tick does not re-emit them.
#[cfg(feature = "imap")]
pub struct ImapInbound {
    host: String,
    port: u16,
    username: String,
    password: String,
    folder: String,
    allow_from: Vec<String>,
    display_name: String,
}

#[cfg(feature = "imap")]
impl ImapInbound {
    /// Construct from config fields.
    ///
    /// # Errors
    ///
    /// Returns `Err` on IMAP connection, auth, or fetch failure.
    #[must_use]
    pub fn new(
        host: String,
        port: u16,
        username: String,
        password: String,
        folder: String,
        allow_from: Vec<String>,
        display_name: String,
    ) -> Self {
        Self { host, port, username, password, folder, allow_from, display_name }
    }

    /// Fetch UNSEEN messages, apply allowlist, return replies.
    ///
    /// Marks accepted messages `\Seen`.  This method is `async`; call
    /// within a tokio runtime.
    ///
    /// NOTE: This is a deferred_acs [8] stub; a live IMAP server is required
    /// for the real round-trip.  The maildir leg above is the autonomous proof.
    ///
    /// # Errors
    ///
    /// Returns `Err` on IMAP connection, auth, or fetch failure.
    pub async fn poll(&self) -> Result<Vec<InboundReply>> {
        // deferred_acs: [8]
        // The async-imap 0.9 integration would:
        //   1. TlsConnector (rustls) → async_imap::connect
        //   2. client.login(&self.username, &self.password)
        //   3. client.select(&self.folder)
        //   4. client.uid_search("UNSEEN")
        //   5. For each uid: fetch headers → parse From → apply allowlist
        //   6. For accepted: fetch body, uid_store(uid, "+FLAGS (\\Seen)")
        let _ = (&self.host, self.port, &self.username, &self.password,
                 &self.folder, &self.allow_from, &self.display_name);
        Ok(Vec::new())
    }
}
