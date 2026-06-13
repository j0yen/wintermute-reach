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
//! - **Device-alive gate** — the nudge fires only when the pulse-watch presence state
//!   confirms `hearing_confirmed_in_window: true`.  A deaf window is suppressed
//!   (handled by pulse-deaf-escalation).  If the state file is absent the nudge
//!   fires (fail-open — don't suppress when we don't know).

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::SilenceNudgeConfig;
use crate::transport::build_transport;
use crate::Config;

// --------------------------------------------------------------------------
// Hearing-gate: read pulse-watch presence state
// --------------------------------------------------------------------------

/// Minimal deserialisation view of pulse-watch's `DailyState`.
///
/// Only the field needed by the hearing gate is extracted; all other fields
/// are ignored.  This avoids a shared crate dependency.
///
/// The state file lives at `~/.local/state/wintermute-presence/state.json`
/// (documented contract with pulse-watch / `wintermute-presence`).
#[derive(Deserialize, Default)]
struct PresenceStateHearingBit {
    #[serde(default)]
    hearing_confirmed_in_window: bool,
}

/// Default path for the pulse-watch presence state file.
///
/// Returns `None` if `$HOME` is not set.
pub fn default_presence_state_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|home| {
        PathBuf::from(home)
            .join(".local/state/wintermute-presence/state.json")
    })
}

/// Read `hearing_confirmed_in_window` from the pulse-watch state file.
///
/// Returns:
/// - `true` if the field is present and `true` in the file.
/// - `false` if the field is present and `false` (device deaf / not confirmed).
/// - `None` if the file is absent or unreadable (fail-open caller should fire nudge).
///
/// Parse errors (corrupt JSON) are treated as absent — the nudge fires rather
/// than silently swallowing a bad file.
pub fn read_hearing_confirmed(presence_state_path: &Path) -> Option<bool> {
    if !presence_state_path.exists() {
        return None;
    }
    let raw = match std::fs::read_to_string(presence_state_path) {
        Ok(s) => s,
        Err(_) => return None,
    };
    let parsed: PresenceStateHearingBit = match serde_json::from_str(&raw) {
        Ok(p) => p,
        Err(_) => return None,
    };
    Some(parsed.hearing_confirmed_in_window)
}

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

/// Attempt to deliver a silence nudge, honouring the debounce guard and the
/// device-alive hearing gate.
///
/// `presence_state_path` is the path to the pulse-watch `state.json` file.
/// Pass `None` to use the default path (`~/.local/state/wintermute-presence/state.json`).
///
/// Returns `Ok(true)` if a nudge was delivered, `Ok(false)` if skipped
/// (disabled, debounced, or suppressed by the deaf gate), or `Err` on
/// state/transport error.
///
/// # Hearing gate semantics
///
/// | `hearing_confirmed_in_window` | result                      |
/// |-------------------------------|------------------------------|
/// | `true`                        | nudge fires (device hearing) |
/// | `false`                       | nudge suppressed, log emitted (`silence_nudge_suppressed_deaf`) |
/// | state file absent / unreadable | nudge fires (fail-open)    |
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
    maybe_deliver_nudge_with_presence(cfg, nudge_cfg, state_dir, window_key, None)
}

/// Like [`maybe_deliver_nudge`] but with an explicit presence state path.
///
/// Passing `Some(path)` overrides the default `~/.local/state/wintermute-presence/state.json`.
/// Passing `None` uses the default.
///
/// This variant is the primary entry point for tests that need to inject a
/// synthetic presence state file.
///
/// # Errors
///
/// Returns `Err` on state I/O or transport failure.
pub fn maybe_deliver_nudge_with_presence(
    cfg: &Config,
    nudge_cfg: &SilenceNudgeConfig,
    state_dir: &Path,
    window_key: &str,
    presence_state_path: Option<&Path>,
) -> Result<bool> {
    if !nudge_cfg.enabled {
        return Ok(false);
    }

    // Device-alive hearing gate (evaluated before debounce to avoid persisting
    // a suppressed window as "nudged").
    let fallback_presence_path;
    let gate_path: &Path = match presence_state_path {
        Some(p) => p,
        None => {
            fallback_presence_path = default_presence_state_path()
                .unwrap_or_else(|| PathBuf::from("/nonexistent"));
            &fallback_presence_path
        }
    };

    match read_hearing_confirmed(gate_path) {
        Some(true) => {
            // Device was confirmed hearing — proceed with nudge.
        }
        Some(false) => {
            // Device was deaf this window — suppress; escalation handles this.
            eprintln!(
                "{{\"level\":\"info\",\"action\":\"silence_nudge_suppressed_deaf\",\"window\":\"{window_key}\",\"msg\":\"silence nudge suppressed: device was not confirmed hearing this window\"}}"
            );
            return Ok(false);
        }
        None => {
            // State file absent or unreadable — fail-open, fire the nudge.
            eprintln!(
                "{{\"level\":\"debug\",\"action\":\"silence_nudge_fail_open\",\"window\":\"{window_key}\",\"msg\":\"presence state absent or unreadable; firing silence nudge (fail-open)\"}}"
            );
        }
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
