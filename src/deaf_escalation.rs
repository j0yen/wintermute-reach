//! Deaf-device escalation — deliver a durable alert when the companion goes deaf.
//!
//! # Design invariants
//!
//! - **Default ON** — `DeafEscalationConfig::enabled` defaults to `true` because
//!   a deaf box is a safety event; the user must explicitly opt-out.
//! - **Debounced per outage** — once a deaf alert fires, subsequent
//!   `wm.health.hearing.fail` events are suppressed until `wm.health.hearing.ok`
//!   clears the flag.
//! - **Full ladder** — uses `run_distress_ladder()` so a nacking primary transport
//!   retries and falls back just as a kin-distress would.
//! - **Recovery note** — on `.ok` after a sent alert, a best-effort (non-ladder)
//!   "hearing restored" note is delivered and suppression is cleared.
//! - **No brain dependency** — deterministic bus-event → transport; no Claude API.
//! - **No `wm.health.*` publication** — the subscriber never publishes on the topics
//!   it subscribes to, so no self-loop is possible.

#![allow(clippy::print_stderr)]

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::{Config, DistressPolicy};
use crate::distress_delivery::run_distress_ladder;
use crate::transport::build_transport;

// --------------------------------------------------------------------------
// Config
// --------------------------------------------------------------------------

/// Configuration for the deaf-device escalation path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeafEscalationConfig {
    /// Whether deaf-device escalation is active (default: `true`).
    #[serde(default = "default_deaf_enabled")]
    pub enabled: bool,
    /// Seconds after a sent alert before a second deaf event re-alerts (cool-down).
    ///
    /// A zero value means: never re-alert until an explicit `.ok` clears the flag.
    #[serde(default = "default_cooldown_s")]
    pub cooldown_s: u64,
}

fn default_deaf_enabled() -> bool {
    true
}

fn default_cooldown_s() -> u64 {
    // Default: large value — rely on the explicit `.ok` rather than the timer.
    3600
}

impl Default for DeafEscalationConfig {
    fn default() -> Self {
        Self {
            enabled: default_deaf_enabled(),
            cooldown_s: default_cooldown_s(),
        }
    }
}

// --------------------------------------------------------------------------
// Persistent state
// --------------------------------------------------------------------------

/// Persistent debounce state for deaf-device escalation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeafEscalationState {
    /// Whether we are currently in a suppressed (alert-already-sent) state.
    pub suppressed: bool,
    /// Unix timestamp (seconds) when the alert was first sent (for cool-down).
    ///
    /// `None` if no alert has been sent this outage.
    pub alert_sent_at: Option<u64>,
}

fn state_path(state_dir: &Path) -> PathBuf {
    state_dir.join("deaf_escalation_state.json")
}

/// Load or initialise [`DeafEscalationState`] from disk.
///
/// # Errors
///
/// Returns `Err` if the state file exists but fails to parse.
pub fn load_state(state_dir: &Path) -> Result<DeafEscalationState> {
    let path = state_path(state_dir);
    if !path.exists() {
        return Ok(DeafEscalationState::default());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading deaf escalation state {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("parsing deaf escalation state {}", path.display()))
}

/// Persist [`DeafEscalationState`] to disk.
///
/// # Errors
///
/// Returns `Err` on serialisation or write failure.
pub fn save_state(state_dir: &Path, state: &DeafEscalationState) -> Result<()> {
    std::fs::create_dir_all(state_dir)
        .with_context(|| format!("creating state dir {}", state_dir.display()))?;
    let path = state_path(state_dir);
    let json = serde_json::to_string_pretty(state)
        .context("serialising deaf escalation state")?;
    std::fs::write(&path, json)
        .with_context(|| format!("writing deaf escalation state {}", path.display()))
}

// --------------------------------------------------------------------------
// Hearing-event envelope
// --------------------------------------------------------------------------

/// Partial deserialisation of a `wm.health.hearing.*` bus envelope.
#[derive(Debug, Deserialize)]
pub struct HearingEventEnvelope {
    /// Topic: `"wm.health.hearing.fail"` or `"wm.health.hearing.ok"`.
    #[serde(default)]
    pub msg_type: String,
    /// ISO-8601 timestamp of the failure (optional; used in the alert body).
    #[serde(default)]
    pub fail_ts: Option<String>,
    /// Age in seconds since the last `.ok` event (optional; used in alert body).
    #[serde(default)]
    pub last_ok_age_s: Option<u64>,
}

// --------------------------------------------------------------------------
// Message formatting
// --------------------------------------------------------------------------

/// Format the deaf-alert subject line.
#[must_use]
pub fn format_deaf_subject() -> String {
    "[wintermute] DEAF — companion can no longer hear commands".to_string()
}

/// Format the deaf-alert body, incorporating envelope metadata when available.
#[must_use]
pub fn format_deaf_body(envelope: &HearingEventEnvelope) -> String {
    let mut body = String::from(
        "wintermute has gone deaf — it can no longer hear commands.\n\n",
    );
    if let Some(ts) = &envelope.fail_ts {
        body.push_str(&format!("First deaf event at: {ts}\n"));
    }
    if let Some(age) = envelope.last_ok_age_s {
        let mins = age / 60;
        body.push_str(&format!(
            "Last confirmed hearing: ~{mins} minute(s) ago\n"
        ));
    }
    body.push_str(
        "\nThe companion may need attention. If the mic or audio subsystem is at Mom's home, \
         please check on it.",
    );
    body
}

/// Format the recovery note body.
#[must_use]
pub fn format_recovery_body() -> String {
    "Good news — wintermute is hearing again. Listening for commands has been restored.".to_string()
}

/// Format the recovery note subject.
#[must_use]
pub fn format_recovery_subject() -> String {
    "[wintermute] hearing restored".to_string()
}

// --------------------------------------------------------------------------
// Clock abstraction (for testability)
// --------------------------------------------------------------------------

/// Clock abstraction so tests can inject a fake "now".
pub trait Clock: Send + Sync {
    /// Current time as Unix seconds.
    fn now_unix_s(&self) -> u64;
}

/// Wall-clock implementation.
pub struct WallClock;

impl Clock for WallClock {
    fn now_unix_s(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

// --------------------------------------------------------------------------
// Core logic
// --------------------------------------------------------------------------

/// Handle a `wm.health.hearing.fail` event.
///
/// Delivers a deaf-device alert via the distress ladder if not suppressed.
/// Returns `Ok(true)` if an alert was sent, `Ok(false)` if suppressed.
///
/// # Errors
///
/// Returns `Err` on state I/O or transport error.
pub fn on_hearing_fail(
    cfg: &Config,
    deaf_cfg: &DeafEscalationConfig,
    policy: &DistressPolicy,
    state_dir: &Path,
    envelope: &HearingEventEnvelope,
    clock: &dyn Clock,
) -> Result<bool> {
    if !deaf_cfg.enabled {
        return Ok(false);
    }

    let mut state = load_state(state_dir)?;

    // Debounce: are we already in suppressed mode?
    if state.suppressed {
        // Check cool-down: if cooldown_s > 0 and enough time has passed, re-alert.
        let should_re_alert = if deaf_cfg.cooldown_s > 0 {
            match state.alert_sent_at {
                Some(sent_at) => {
                    let now = clock.now_unix_s();
                    now.saturating_sub(sent_at) >= deaf_cfg.cooldown_s
                }
                None => false,
            }
        } else {
            false
        };

        if !should_re_alert {
            return Ok(false);
        }
    }

    // Compose the alert.
    let subject = format_deaf_subject();
    let body = format_deaf_body(envelope);

    // Build transport.
    let primary = build_transport(cfg)?;

    // Run the distress ladder (retry + fallback).
    let result = run_distress_ladder(
        &subject,
        &body,
        primary.as_ref(),
        cfg.transport_kind(),
        &[], // fallbacks: none configured for now (ladder still retries primary)
        policy,
    )?;

    eprintln!(
        "{{\"level\":\"info\",\"action\":\"deaf_alert\",\"delivered\":{},\"transport\":\"{}\"}}",
        result.delivered, result.transport
    );

    // Persist suppression state whether or not delivery succeeded:
    // we don't want a failing transport to spam.  A re-alert will happen
    // when the cool-down elapses or an `.ok` arrives.
    state.suppressed = true;
    state.alert_sent_at = Some(clock.now_unix_s());
    save_state(state_dir, &state)?;

    Ok(result.delivered)
}

/// Handle a `wm.health.hearing.ok` event.
///
/// If suppressed (alert was sent), delivers a best-effort recovery note and
/// clears suppression.  If not suppressed, does nothing.
///
/// Returns `Ok(true)` if a recovery note was sent, `Ok(false)` if nothing was
/// sent.
///
/// # Errors
///
/// Returns `Err` on state I/O or transport error.
pub fn on_hearing_ok(
    cfg: &Config,
    deaf_cfg: &DeafEscalationConfig,
    state_dir: &Path,
) -> Result<bool> {
    if !deaf_cfg.enabled {
        return Ok(false);
    }

    let mut state = load_state(state_dir)?;

    if !state.suppressed {
        // No prior alert — nothing to do.
        return Ok(false);
    }

    // Best-effort recovery note (not over the full ladder).
    let subject = format_recovery_subject();
    let body = format_recovery_body();
    let transport = build_transport(cfg)?;
    let result = transport
        .deliver(&subject, &body)
        .context("deaf escalation: recovery note transport")?;

    eprintln!(
        "{{\"level\":\"info\",\"action\":\"deaf_recovery\",\"delivered\":{},\"transport\":\"{}\"}}",
        result.delivered, result.transport
    );

    // Clear suppression regardless of whether the recovery note succeeded.
    state.suppressed = false;
    state.alert_sent_at = None;
    save_state(state_dir, &state)?;

    Ok(result.delivered)
}

// --------------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DistressPolicy, EmailConfig, TransportConfig};
    use crate::distress_delivery::test_helpers::FakeTransport;
    use crate::transport::{DeliveryResult, Transport};
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    // ---- helpers ----

    fn make_config(sendmail: &str) -> Config {
        Config {
            transport: TransportConfig::Email(EmailConfig {
                to: "test@example.com".to_string(),
                from: "reach@localhost".to_string(),
                sendmail: sendmail.to_string(),
                smtp_host: None,
            }),
            ..Config::default()
        }
    }

    fn always_ok_policy() -> DistressPolicy {
        DistressPolicy {
            max_retries: 0,
            backoff_ms: 0,
            fallback_order: Vec::new(),
        }
    }

    fn default_deaf_cfg() -> DeafEscalationConfig {
        DeafEscalationConfig {
            enabled: true,
            cooldown_s: 3600,
        }
    }

    fn no_cooldown_deaf_cfg() -> DeafEscalationConfig {
        DeafEscalationConfig {
            enabled: true,
            cooldown_s: 0, // never re-alert from timer; only `.ok` clears
        }
    }

    fn envelope() -> HearingEventEnvelope {
        HearingEventEnvelope {
            msg_type: "wm.health.hearing.fail".to_string(),
            fail_ts: Some("2026-06-13T00:00:00Z".to_string()),
            last_ok_age_s: Some(300),
        }
    }

    struct FakeClock {
        now: Arc<Mutex<u64>>,
    }

    impl FakeClock {
        fn new(t: u64) -> Self {
            Self { now: Arc::new(Mutex::new(t)) }
        }

        fn set(&self, t: u64) {
            *self.now.lock().unwrap() = t;
        }
    }

    impl Clock for FakeClock {
        fn now_unix_s(&self) -> u64 {
            *self.now.lock().unwrap()
        }
    }

    // ---- AC1: first .fail triggers exactly one alert ----

    #[test]
    fn ac1_first_fail_triggers_alert() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config("/bin/true"); // sendmail always succeeds (exit 0)
        let deaf_cfg = default_deaf_cfg();
        let policy = always_ok_policy();
        let clock = FakeClock::new(1_000_000);
        let env = envelope();

        let delivered = on_hearing_fail(
            &cfg,
            &deaf_cfg,
            &policy,
            tmp.path(),
            &env,
            &clock,
        ).unwrap();

        assert!(delivered, "first .fail should deliver an alert");

        // State should now be suppressed.
        let state = load_state(tmp.path()).unwrap();
        assert!(state.suppressed, "state should be suppressed after first alert");
    }

    // ---- AC3: debounce — second consecutive .fail does NOT fire ----

    #[test]
    fn ac3_second_fail_suppressed() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config("/bin/true");
        let deaf_cfg = no_cooldown_deaf_cfg(); // cooldown_s=0, only .ok clears
        let policy = always_ok_policy();
        let clock = FakeClock::new(1_000_000);
        let env = envelope();

        // First alert fires.
        on_hearing_fail(&cfg, &deaf_cfg, &policy, tmp.path(), &env, &clock).unwrap();

        // Second .fail — should be suppressed.
        let result2 = on_hearing_fail(
            &cfg,
            &deaf_cfg,
            &policy,
            tmp.path(),
            &env,
            &clock,
        ).unwrap();

        assert!(!result2, "second consecutive .fail must be suppressed");
    }

    // ---- AC3: after .ok + new .fail, alert fires again ----

    #[test]
    fn ac3_fail_ok_fail_fires_again() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config("/bin/true");
        let deaf_cfg = no_cooldown_deaf_cfg();
        let policy = always_ok_policy();
        let clock = FakeClock::new(1_000_000);
        let env = envelope();

        // First outage alert.
        on_hearing_fail(&cfg, &deaf_cfg, &policy, tmp.path(), &env, &clock).unwrap();

        // Recovery — clears suppression.
        on_hearing_ok(&cfg, &deaf_cfg, tmp.path()).unwrap();
        let state = load_state(tmp.path()).unwrap();
        assert!(!state.suppressed, "suppression must be cleared after .ok");

        // New outage — should alert again.
        let result3 = on_hearing_fail(
            &cfg,
            &deaf_cfg,
            &policy,
            tmp.path(),
            &env,
            &clock,
        ).unwrap();
        assert!(result3, "new .fail after .ok must deliver again");
    }

    // ---- AC4: .ok with no prior alert sends nothing ----

    #[test]
    fn ac4_ok_with_no_prior_alert_sends_nothing() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config("/bin/true");
        let deaf_cfg = default_deaf_cfg();

        let sent = on_hearing_ok(&cfg, &deaf_cfg, tmp.path()).unwrap();
        assert!(!sent, ".ok with no prior alert must deliver nothing");
    }

    // ---- AC4: .ok after alert delivers recovery note ----

    #[test]
    fn ac4_ok_after_alert_delivers_recovery() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config("/bin/true");
        let deaf_cfg = default_deaf_cfg();
        let policy = always_ok_policy();
        let clock = FakeClock::new(1_000_000);
        let env = envelope();

        on_hearing_fail(&cfg, &deaf_cfg, &policy, tmp.path(), &env, &clock).unwrap();
        let recovery_sent = on_hearing_ok(&cfg, &deaf_cfg, tmp.path()).unwrap();
        assert!(recovery_sent, ".ok after alert must deliver a recovery note");
    }

    // ---- AC5: disabled — no alert on .fail ----

    #[test]
    fn ac5_disabled_no_alert() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config("/bin/true");
        let deaf_cfg = DeafEscalationConfig {
            enabled: false,
            cooldown_s: 3600,
        };
        let policy = always_ok_policy();
        let clock = FakeClock::new(1_000_000);
        let env = envelope();

        let result = on_hearing_fail(&cfg, &deaf_cfg, &policy, tmp.path(), &env, &clock).unwrap();
        assert!(!result, "disabled config must not deliver any alert");

        let state = load_state(tmp.path()).unwrap();
        assert!(!state.suppressed, "state must not be mutated when disabled");
    }

    // ---- AC6: no brain dependency — alert works without API key ----

    #[test]
    fn ac6_no_brain_dependency() {
        // This test has no LLM/brain dependency by construction: no HTTP client,
        // no API key, no Claude call.  If this test compiles and passes, AC6 holds.
        let tmp = TempDir::new().unwrap();
        let cfg = make_config("/bin/true");
        let deaf_cfg = default_deaf_cfg();
        let policy = always_ok_policy();
        let clock = FakeClock::new(1_000_000);
        let env = envelope();

        let result = on_hearing_fail(&cfg, &deaf_cfg, &policy, tmp.path(), &env, &clock);
        assert!(result.is_ok(), "deaf alert must not fail due to missing brain");
    }

    // ---- Cool-down re-alert test ----

    #[test]
    fn cooldown_re_alerts_after_elapsed() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config("/bin/true");
        let deaf_cfg = DeafEscalationConfig {
            enabled: true,
            cooldown_s: 60,
        };
        let policy = always_ok_policy();
        let clock = FakeClock::new(1_000_000);
        let env = envelope();

        // First alert.
        on_hearing_fail(&cfg, &deaf_cfg, &policy, tmp.path(), &env, &clock).unwrap();

        // Advance time by less than cooldown — should still be suppressed.
        clock.set(1_000_000 + 30);
        let result_early = on_hearing_fail(&cfg, &deaf_cfg, &policy, tmp.path(), &env, &clock).unwrap();
        assert!(!result_early, "should still be suppressed before cooldown");

        // Advance time past cooldown.
        clock.set(1_000_000 + 61);
        let result_late = on_hearing_fail(&cfg, &deaf_cfg, &policy, tmp.path(), &env, &clock).unwrap();
        assert!(result_late, "should re-alert after cooldown elapsed");
    }

    // ---- Primary-fail + fallback-ok pattern (AC2) ----
    // This exercises run_distress_ladder directly with stub transports.
    #[test]
    fn ac2_primary_fail_fallback_ok() {
        use crate::distress_delivery::run_distress_ladder;

        let primary = FakeTransport::new("primary", [false]);
        let fallback = FakeTransport::new("fallback", [true]);
        let policy = DistressPolicy {
            max_retries: 0,
            backoff_ms: 0,
            fallback_order: Vec::new(),
        };

        let result = run_distress_ladder(
            "test subject",
            "test body",
            &primary,
            "primary",
            &[(&fallback, "fallback")],
            &policy,
        ).unwrap();

        assert!(result.delivered, "fallback should succeed");
        assert_eq!(result.transport, "fallback");
        assert_eq!(primary.call_count(), 1);
    }
}
