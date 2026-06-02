//! Digest AC7: The tally resets at the day boundary and the reset is logged (test).

use wintermute_reach::digest::{PresenceTally, is_new_day, today_utc};

/// AC7: reset() clears all fields and sets the new date.
#[test]
fn digest_ac7_reset_clears_tally() {
    let mut tally = PresenceTally::default();
    tally.date = "2024-01-01".to_string();
    tally.record_summon(1_717_000_000);
    tally.record_summon(1_717_003_600);
    tally.record_silence();

    assert_eq!(tally.interaction_count, 2);
    assert!(tally.silence_flagged);

    tally.reset("2024-01-02");

    assert_eq!(tally.interaction_count, 0, "count should reset to 0");
    assert_eq!(tally.first_ts, 0, "first_ts should reset to 0");
    assert_eq!(tally.last_ts, 0, "last_ts should reset to 0");
    assert!(!tally.silence_flagged, "silence_flagged should reset");
    assert_eq!(tally.date, "2024-01-02", "date should be updated to new day");
}

/// AC7: is_new_day detects stale tally (different date).
#[test]
fn digest_ac7_is_new_day_detects_stale() {
    let tally = PresenceTally {
        date: "1970-01-01".to_string(),
        ..Default::default()
    };
    assert!(
        is_new_day(&tally),
        "stale tally (1970-01-01) should be detected as new day"
    );
}

/// AC7: is_new_day returns false for today's date.
#[test]
fn digest_ac7_is_new_day_false_for_today() {
    let tally = PresenceTally {
        date: today_utc(),
        ..Default::default()
    };
    assert!(
        !is_new_day(&tally),
        "today's tally should not be a new day"
    );
}

/// AC7: is_new_day returns true for empty date (fresh default tally).
#[test]
fn digest_ac7_is_new_day_true_for_empty_date() {
    let tally = PresenceTally::default();
    assert!(
        is_new_day(&tally),
        "empty date should be treated as new day"
    );
}

/// AC7: After reset, further summons accumulate on the new date.
#[test]
fn digest_ac7_summons_accumulate_after_reset() {
    let mut tally = PresenceTally::default();
    tally.date = "2024-01-01".to_string();
    tally.record_summon(1_700_000_000);

    tally.reset("2024-01-02");
    tally.record_summon(1_700_086_400);
    tally.record_summon(1_700_090_000);

    assert_eq!(
        tally.interaction_count, 2,
        "summons after reset should start from 0"
    );
    assert_eq!(tally.date, "2024-01-02");
}
