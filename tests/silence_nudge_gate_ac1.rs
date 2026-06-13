//! pulse-silence-gate acceptance tests.
//!
//! AC-gate-1: hearing_confirmed=true  → nudge fires.
//! AC-gate-2: hearing_confirmed=false → nudge suppressed (device deaf).
//! AC-gate-3: presence state file absent → nudge fires (fail-open).
//! AC-gate-4: debounce guard still preserved when gate passes (hearing=true, same window).
//! AC-gate-5: disabled nudge → no delivery even when hearing=true (feature-disabled unchanged).
//! AC-gate-6: integration — write pulse-watch state, read through gate.

use std::io::Write as _;
use tempfile::TempDir;
use wintermute_reach::{
    SilenceNudgeConfig,
    config::{EmailConfig, TransportConfig},
    silence_nudge::maybe_deliver_nudge_with_presence,
};

fn make_capture_sendmail(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let capture = dir.join("capture.txt");
    let script = dir.join("sendmail.sh");
    let content = format!("#!/bin/sh\ncat >> {}\n", capture.display());
    std::fs::write(&script, &content).expect("write script");
    {
        use std::os::unix::fs::PermissionsExt as _;
        let meta = std::fs::metadata(&script).expect("meta");
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).expect("chmod");
    }
    (script, capture)
}

fn make_cfg(script: &std::path::Path) -> wintermute_reach::Config {
    wintermute_reach::Config {
        transport: TransportConfig::Email(EmailConfig {
            to: "joe@example.com".to_string(),
            from: "wintermute@localhost".to_string(),
            sendmail: script.to_str().expect("path").to_string(),
            smtp_host: None,
        }),
        from: "wintermute".to_string(),
        ..Default::default()
    }
}

fn enabled_nudge() -> SilenceNudgeConfig {
    SilenceNudgeConfig {
        enabled: true,
        contact_name: "Mom".to_string(),
    }
}

/// Write a presence state JSON file with the given `hearing_confirmed_in_window`.
fn write_presence_state(dir: &std::path::Path, hearing_confirmed: bool) -> std::path::PathBuf {
    let path = dir.join("state.json");
    let json = format!(
        r#"{{"date":"2026-06-13","daily_count":0,"last_interaction_ts":null,"silence_emitted_for_window":false,"hearing_confirmed_in_window":{}}}"#,
        hearing_confirmed
    );
    std::fs::write(&path, json).expect("write presence state");
    path
}

/// AC-gate-1: hearing_confirmed=true → nudge fires.
#[test]
fn gate_ac1_hearing_confirmed_true_fires_nudge() {
    let state_dir = TempDir::new().expect("state_dir");
    let presence_dir = TempDir::new().expect("presence_dir");
    let (script, capture) = make_capture_sendmail(state_dir.path());
    let cfg = make_cfg(&script);
    let nudge_cfg = enabled_nudge();

    let presence_path = write_presence_state(presence_dir.path(), true);

    let delivered =
        maybe_deliver_nudge_with_presence(&cfg, &nudge_cfg, state_dir.path(), "2026-06-13", Some(&presence_path))
            .expect("deliver");

    assert!(delivered, "nudge must fire when hearing_confirmed=true");
    let captured = std::fs::read_to_string(&capture).expect("capture");
    assert!(!captured.is_empty(), "sendmail must have been called");
}

/// AC-gate-2: hearing_confirmed=false → nudge suppressed (device deaf).
#[test]
fn gate_ac2_hearing_confirmed_false_suppresses_nudge() {
    let state_dir = TempDir::new().expect("state_dir");
    let presence_dir = TempDir::new().expect("presence_dir");
    let (script, capture) = make_capture_sendmail(state_dir.path());
    let cfg = make_cfg(&script);
    let nudge_cfg = enabled_nudge();

    let presence_path = write_presence_state(presence_dir.path(), false);

    let delivered =
        maybe_deliver_nudge_with_presence(&cfg, &nudge_cfg, state_dir.path(), "2026-06-13", Some(&presence_path))
            .expect("no error expected on suppress");

    assert!(!delivered, "nudge must be suppressed when hearing_confirmed=false (deaf)");
    let captured = std::fs::read_to_string(&capture).unwrap_or_default();
    assert!(
        captured.is_empty(),
        "sendmail must NOT have been called on deaf suppression"
    );
}

/// AC-gate-3: presence state file absent → nudge fires (fail-open).
#[test]
fn gate_ac3_absent_state_file_fires_nudge_fail_open() {
    let state_dir = TempDir::new().expect("state_dir");
    let (script, capture) = make_capture_sendmail(state_dir.path());
    let cfg = make_cfg(&script);
    let nudge_cfg = enabled_nudge();

    // Point at a path that does not exist.
    let nonexistent = std::path::Path::new("/nonexistent/wintermute-presence/state.json");

    let delivered =
        maybe_deliver_nudge_with_presence(&cfg, &nudge_cfg, state_dir.path(), "2026-06-13", Some(nonexistent))
            .expect("deliver");

    assert!(delivered, "nudge must fire (fail-open) when state file absent");
    let captured = std::fs::read_to_string(&capture).expect("capture");
    assert!(!captured.is_empty(), "sendmail must have been called");
}

/// AC-gate-4: debounce guard preserved after gate passes (hearing=true, same window).
#[test]
fn gate_ac4_debounce_preserved_after_gate_passes() {
    let state_dir = TempDir::new().expect("state_dir");
    let presence_dir = TempDir::new().expect("presence_dir");
    let (script, _capture) = make_capture_sendmail(state_dir.path());
    let cfg = make_cfg(&script);
    let nudge_cfg = enabled_nudge();

    let presence_path = write_presence_state(presence_dir.path(), true);

    // First call: should fire.
    let first =
        maybe_deliver_nudge_with_presence(&cfg, &nudge_cfg, state_dir.path(), "2026-06-13", Some(&presence_path))
            .expect("first deliver");
    assert!(first, "first nudge should fire");

    // Second call for same window: debounce should block.
    let second =
        maybe_deliver_nudge_with_presence(&cfg, &nudge_cfg, state_dir.path(), "2026-06-13", Some(&presence_path))
            .expect("second deliver");
    assert!(!second, "second nudge for same window must be debounced");
}

/// AC-gate-5: disabled nudge → no delivery even when hearing=true.
#[test]
fn gate_ac5_disabled_nudge_no_delivery_even_when_hearing() {
    let state_dir = TempDir::new().expect("state_dir");
    let presence_dir = TempDir::new().expect("presence_dir");
    let (script, capture) = make_capture_sendmail(state_dir.path());
    let cfg = make_cfg(&script);
    let nudge_cfg = SilenceNudgeConfig::default(); // enabled = false

    let presence_path = write_presence_state(presence_dir.path(), true);

    let delivered =
        maybe_deliver_nudge_with_presence(&cfg, &nudge_cfg, state_dir.path(), "2026-06-13", Some(&presence_path))
            .expect("no error");

    assert!(!delivered, "disabled nudge must not deliver even when hearing=true");
    let captured = std::fs::read_to_string(&capture).unwrap_or_default();
    assert!(captured.is_empty(), "sendmail must not be called when nudge disabled");
}

/// AC-gate-6: integration — write pulse-watch state, read through gate.
///
/// Simulates the full contract: pulse-watch writes `state.json` with the
/// agreed field; reach reads it; the gate fires/suppresses correctly.
#[test]
fn gate_ac6_integration_write_presence_read_through_gate() {
    let state_dir = TempDir::new().expect("state_dir");
    let presence_dir = TempDir::new().expect("presence_dir");

    // Simulate pulse-watch writing state (hearing confirmed today).
    let state_json = r#"{
        "date": "2026-06-13",
        "daily_count": 3,
        "last_interaction_ts": "2026-06-13T10:00:00Z",
        "silence_emitted_for_window": true,
        "hearing_confirmed_in_window": true
    }"#;
    let state_path = presence_dir.path().join("state.json");
    std::fs::write(&state_path, state_json).expect("write pulse-watch state");

    let (script, capture) = make_capture_sendmail(state_dir.path());
    let cfg = make_cfg(&script);
    let nudge_cfg = enabled_nudge();

    let delivered =
        maybe_deliver_nudge_with_presence(&cfg, &nudge_cfg, state_dir.path(), "2026-06-13", Some(&state_path))
            .expect("deliver");

    assert!(delivered, "integration: nudge should fire when pulse-watch confirms hearing");
    let captured = std::fs::read_to_string(&capture).expect("capture");
    assert!(captured.contains("Mom"), "delivery body should contain contact name");

    // Now simulate deaf case: overwrite with hearing_confirmed=false.
    let deaf_state_json = r#"{
        "date": "2026-06-13",
        "daily_count": 0,
        "last_interaction_ts": null,
        "silence_emitted_for_window": true,
        "hearing_confirmed_in_window": false
    }"#;
    // Use a new state_dir so debounce doesn't interfere.
    let state_dir2 = TempDir::new().expect("state_dir2");
    let (script2, capture2) = make_capture_sendmail(state_dir2.path());
    let cfg2 = make_cfg(&script2);
    let deaf_state_path = presence_dir.path().join("deaf_state.json");
    std::fs::write(&deaf_state_path, deaf_state_json).expect("write deaf state");

    let deaf_delivered =
        maybe_deliver_nudge_with_presence(&cfg2, &nudge_cfg, state_dir2.path(), "2026-06-13", Some(&deaf_state_path))
            .expect("deaf deliver");

    assert!(!deaf_delivered, "integration: nudge must be suppressed when device was deaf");
    let captured2 = std::fs::read_to_string(&capture2).unwrap_or_default();
    assert!(captured2.is_empty(), "no sendmail call when deaf");
}
