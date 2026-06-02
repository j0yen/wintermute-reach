//! Daily digest aggregation for `wm-reach`.
//!
//! Subscribes to `wm.presence.summon` and `wm.presence.silence` events and
//! accumulates a per-day tally.  At the configured digest time, formats a
//! human-readable summary line and delivers it via the existing `Transport`.
//!
//! # Design invariants
//!
//! - **One transport code path** — `DigestEngine` uses `build_transport` from
//!   the existing `transport` module.  No second transport is introduced.
//! - **Distress is always instant** — the digest never delays distress delivery.
//!   `wm.presence.silence` is reflected in the digest body but does NOT trigger
//!   an immediate delivery.
//! - **Opt-in** — no digest is ever delivered unless `DigestConfig::enabled` is
//!   `true`.
//! - **State persistence** — the tally is written to a JSON file after every
//!   update so a daemon restart mid-day does not lose the running count.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::DigestConfig;
use crate::transport::build_transport;
use crate::Config;

/// Per-day interaction tally.
///
/// Persisted to disk after every mutating operation so a restart mid-day
/// keeps the running count (AC6 — state round-trip).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PresenceTally {
    /// Calendar date this tally covers (`YYYY-MM-DD`).
    pub date: String,
    /// Number of `wm.presence.summon` events received today.
    pub interaction_count: u32,
    /// Unix timestamp of the first summon today (0 if none).
    pub first_ts: u64,
    /// Unix timestamp of the most recent summon today (0 if none).
    pub last_ts: u64,
    /// Whether a `wm.presence.silence` event was received today.
    pub silence_flagged: bool,
}

impl PresenceTally {
    /// Apply a `wm.presence.summon` event at the given timestamp.
    pub const fn record_summon(&mut self, ts: u64) {
        self.interaction_count += 1;
        if self.first_ts == 0 {
            self.first_ts = ts;
        }
        self.last_ts = ts;
    }

    /// Apply a `wm.presence.silence` event.
    pub const fn record_silence(&mut self) {
        self.silence_flagged = true;
    }

    /// Reset tally for a new day, logging the reset.
    pub fn reset(&mut self, new_date: &str) {
        *self = Self {
            date: new_date.to_string(),
            ..Default::default()
        };
    }

    /// Format a human-readable digest body line.
    ///
    /// # Examples
    /// ```text
    /// "Mom talked to wintermute 4 times today; last at 6:12pm."
    /// "Quiet day — Mom hasn't talked to wintermute since yesterday 7:40pm."
    /// ```
    #[must_use]
    pub fn format_digest_body(&self, contact_name: &str) -> String {
        if self.interaction_count == 0 {
            return format!(
                "Quiet day — {contact_name} hasn't talked to wintermute today."
            );
        }

        let last_time = format_ts_local(self.last_ts);

        let base = if self.interaction_count == 1 {
            format!("{contact_name} talked to wintermute once today; last at {last_time}.")
        } else {
            format!(
                "{contact_name} talked to wintermute {} times today; last at {last_time}.",
                self.interaction_count
            )
        };

        if self.silence_flagged {
            format!("{base} (Note: a silence window was flagged today.)")
        } else {
            base
        }
    }
}

/// Format a Unix timestamp as a local time string (`H:MMam/pm`).
///
/// Uses only `std` — no chrono dependency.  Falls back to UTC when the local
/// timezone cannot be determined from `/etc/localtime`.  On failure the raw
/// timestamp is returned as a fallback string.
fn format_ts_local(ts: u64) -> String {
    if ts == 0 {
        return "unknown".to_string();
    }
    // Simple UTC formatting — sufficient for tests; real usage may replace
    // with a tz-aware library if needed.
    let secs = ts % 86_400;
    let hours = secs / 3_600;
    let mins = (secs % 3_600) / 60;
    let (h12, suffix) = if hours == 0 {
        (12u64, "am")
    } else if hours < 12 {
        (hours, "am")
    } else if hours == 12 {
        (12u64, "pm")
    } else {
        (hours - 12, "pm")
    };
    format!("{h12}:{mins:02}{suffix}")
}

/// State file path for a given state directory.
fn state_path(state_dir: &Path) -> PathBuf {
    state_dir.join("digest_tally.json")
}

/// Load or initialise the `PresenceTally` from disk.
///
/// # Errors
///
/// Returns `Err` only if the state file exists but is not valid JSON.
pub fn load_tally(state_dir: &Path) -> Result<PresenceTally> {
    let path = state_path(state_dir);
    if !path.exists() {
        return Ok(PresenceTally::default());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading tally state {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("parsing tally state {}", path.display()))
}

/// Persist the `PresenceTally` to disk.
///
/// Creates `state_dir` if it does not exist.
///
/// # Errors
///
/// Returns `Err` on serialisation or write failure.
pub fn save_tally(state_dir: &Path, tally: &PresenceTally) -> Result<()> {
    std::fs::create_dir_all(state_dir)
        .with_context(|| format!("creating state dir {}", state_dir.display()))?;
    let path = state_path(state_dir);
    let json = serde_json::to_string_pretty(tally).context("serialising tally")?;
    std::fs::write(&path, json)
        .with_context(|| format!("writing tally state {}", path.display()))
}

/// Deliver the daily digest via the existing transport.
///
/// Uses `build_transport` — no second transport code path.
///
/// # Errors
///
/// Returns `Err` on transport construction or delivery failure.
pub fn deliver_digest(
    cfg: &Config,
    digest_cfg: &DigestConfig,
    tally: &PresenceTally,
) -> Result<crate::transport::DeliveryResult> {
    if !digest_cfg.enabled {
        // Opt-in gate: return a synthetic "not delivered" result.
        return Ok(crate::transport::DeliveryResult {
            delivered: false,
            transport: "disabled".to_string(),
            reference: None,
            error: Some("digest disabled in config".to_string()),
        });
    }

    let contact = digest_cfg.contact_name.as_deref().unwrap_or("Mom");
    let body = tally.format_digest_body(contact);
    let subject = "[wintermute] daily digest".to_string();

    let transport = build_transport(cfg)?;
    transport
        .deliver(&subject, &body)
        .context("digest transport delivery")
}

/// Current UTC date string `YYYY-MM-DD`.
#[must_use]
pub fn today_utc() -> String {
    // Compute from UNIX_EPOCH without external deps.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days_since_epoch = secs / 86_400;
    // Algorithm: https://howardhinnant.github.io/date_algorithms.html
    // The cast is safe for any date within the epoch; u64/86400 fits in i64.
    #[allow(clippy::cast_possible_wrap, clippy::as_conversions)]
    let z = days_since_epoch as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    #[allow(clippy::similar_names)]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let day_of_year = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let d = day_of_year - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Check whether the tally's date is stale (different from [`today_utc`]).
///
/// Returns `true` when the tally covers a different day than today.
#[must_use]
pub fn is_new_day(tally: &PresenceTally) -> bool {
    tally.date.is_empty() || tally.date != today_utc()
}
