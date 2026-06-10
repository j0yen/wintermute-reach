//! Silence nudge — a single, debounced, gentle delivery when a silence window fires.
//!
//! # Design invariants
//!
//! - **Default OFF** — `SilenceNudgeConfig::enabled` is `false` by default.
//!   A received `wm.presence.silence` is ignored unless the nudge is enabled.
//! - **Exactly one nudge per window** — the debounce guard persists the last-nudged
//!   window key to disk; a daemon restart replaying the same event does not re-nudge.
//! - **Never an alarm** — uses the normal (non-distress) transport path, soft phrasing,
//!   no repeat, no escalation.
//! - **Digest coexistence** — when both digest and nudge are enabled, both fire
//!   independently (belt-and-suspenders for a safety-adjacent signal).

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::SilenceNudgeConfig;
use crate::transport::build_transport;
use crate::Config;

/// Persistent debounce state for the silence nudge.
///
/// Stored as JSON next to the digest tally so it survives restarts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SilenceNudgeState {
    /// The window key for which we last fired a nudge.
    ///
    /// The key is an opaque string derived from the `window` field of the
    /// `wm.presence.silence` event (typically a date string or ISO window id).
    /// An empty string means no nudge has ever been fired.
    pub last_nudged_window: String,
}

// --------------------------------------------------------------------------
// State persistence
// --------------------------------------------------------------------------

fn state_path(state_dir: &Path) -> PathBuf {
    state_dir.join("silence_nudge_state.json")
}

/// Load or initialise the [`SilenceNudgeState`] from disk.
///
/// # Errors
///
/// Returns `Err` only if the state file exists but fails to parse.
pub fn load_state(state_dir: &Path) -> Result<SilenceNudgeState> {
    let path = state_path(state_dir);
    if !path.exists() {
        return Ok(SilenceNudgeState::default());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading silence nudge state {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("parsing silence nudge state {}", path.display()))
}

/// Persist the [`SilenceNudgeState`] to disk.
///
/// Creates `state_dir` if it does not exist.
///
/// # Errors
///
/// Returns `Err` on serialisation or write failure.
pub fn save_state(state_dir: &Path, state: &SilenceNudgeState) -> Result<()> {
    std::fs::create_dir_all(state_dir)
        .with_context(|| format!("creating state dir {}", state_dir.display()))?;
    let path = state_path(state_dir);
    let json = serde_json::to_string_pretty(state).context("serialising silence nudge state")?;
    std::fs::write(&path, json)
        .with_context(|| format!("writing silence nudge state {}", path.display()))
}

// --------------------------------------------------------------------------
// Nudge delivery
// --------------------------------------------------------------------------

/// Format the gentle nudge body.
#[must_use]
pub fn format_nudge_body(contact_name: &str) -> String {
    format!("Haven't heard from {contact_name} today.")
}

/// Attempt to deliver a silence nudge, honouring the debounce guard.
///
/// Returns `Ok(true)` if a nudge was delivered, `Ok(false)` if skipped
/// (disabled or already nudged for this window), or `Err` on state/transport error.
///
/// # Errors
///
/// Returns `Err` on state I/O or transport failure.
pub fn maybe_deliver_nudge(
    cfg: &Config,
    nudge_cfg: &SilenceNudgeConfig,
    state_dir: &Path,
    window_key: &str,
) -> Result<bool> {
    if !nudge_cfg.enabled {
        return Ok(false);
    }

    let mut state = load_state(state_dir)?;

    // Debounce: already nudged for this window?
    if state.last_nudged_window == window_key {
        return Ok(false);
    }

    let body = format_nudge_body(&nudge_cfg.contact_name);
    let subject = "[wintermute] silence nudge".to_string();

    let transport = build_transport(cfg)?;
    let result = transport
        .deliver(&subject, &body)
        .context("silence nudge transport delivery")?;

    if result.delivered {
        // Only persist the debounce state on successful delivery.
        state.last_nudged_window = window_key.to_string();
        save_state(state_dir, &state)?;
        Ok(true)
    } else {
        // Delivery failed — don't persist the window key so a retry is possible.
        Err(anyhow::anyhow!(
            "silence nudge transport failed: {}",
            result.error.as_deref().unwrap_or("unknown")
        ))
    }
}
