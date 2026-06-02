//! Digest AC6: The per-day tally survives a daemon restart (state round-trip test).

use wintermute_reach::digest::{PresenceTally, load_tally, save_tally};

/// AC6: Tally persists and loads back with identical fields.
#[test]
fn digest_ac6_tally_survives_restart() {
    let dir = tempfile::tempdir().expect("tempdir");

    let mut tally = PresenceTally::default();
    tally.date = "2024-05-28".to_string();
    tally.record_summon(1_716_883_200);
    tally.record_summon(1_716_886_800);
    tally.record_summon(1_716_890_400);
    tally.record_silence();

    // Simulate daemon writing state.
    save_tally(dir.path(), &tally).expect("save_tally");

    // Simulate daemon restart and loading state.
    let loaded = load_tally(dir.path()).expect("load_tally");

    assert_eq!(loaded.date, tally.date, "date should match after round-trip");
    assert_eq!(
        loaded.interaction_count, tally.interaction_count,
        "interaction_count should match"
    );
    assert_eq!(loaded.first_ts, tally.first_ts, "first_ts should match");
    assert_eq!(loaded.last_ts, tally.last_ts, "last_ts should match");
    assert_eq!(
        loaded.silence_flagged, tally.silence_flagged,
        "silence_flagged should match"
    );
}

/// AC6: load_tally returns default when no state file exists.
#[test]
fn digest_ac6_load_returns_default_when_absent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let tally = load_tally(dir.path()).expect("load_tally should not err when absent");

    assert_eq!(tally.interaction_count, 0);
    assert_eq!(tally.first_ts, 0);
    assert!(!tally.silence_flagged);
}

/// AC6: save_tally creates state dir if missing.
#[test]
fn digest_ac6_save_creates_state_dir() {
    let base = tempfile::tempdir().expect("tempdir");
    let state_dir = base.path().join("subdir/reach-state");

    let tally = PresenceTally {
        date: "2024-05-28".to_string(),
        interaction_count: 1,
        first_ts: 1_716_883_200,
        last_ts: 1_716_883_200,
        silence_flagged: false,
    };

    save_tally(&state_dir, &tally).expect("save_tally creates dir");
    assert!(state_dir.exists(), "state_dir should be created");

    let loaded = load_tally(&state_dir).expect("load after create");
    assert_eq!(loaded.interaction_count, 1);
}
