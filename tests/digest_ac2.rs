//! Digest AC2: Four `wm.presence.summon` events across a simulated day produce
//! a tally of 4 with correct first/last timestamps.

use wintermute_reach::digest::PresenceTally;

/// AC2: Four summons produce count=4, correct first/last.
#[test]
fn digest_ac2_four_summons_produce_correct_tally() {
    let mut tally = PresenceTally::default();

    let timestamps = [
        1_717_000_000u64,
        1_717_003_600,
        1_717_007_200,
        1_717_010_800,
    ];

    for &ts in &timestamps {
        tally.record_summon(ts);
    }

    assert_eq!(
        tally.interaction_count, 4,
        "four summons should yield count=4, got {}", tally.interaction_count
    );
    assert_eq!(
        tally.first_ts, timestamps[0],
        "first_ts should be the earliest timestamp"
    );
    assert_eq!(
        tally.last_ts, *timestamps.last().expect("last"),
        "last_ts should be the most recent timestamp"
    );
}

/// AC2: Out-of-order timestamps — first_ts is the first *recorded*, not the minimum.
///
/// The PRD says "first/last interaction timestamps"; in practice presence events
/// arrive chronologically.  The tally tracks recording order as primary,
/// which is the natural streaming behaviour.
#[test]
fn digest_ac2_first_ts_is_first_recorded() {
    let mut tally = PresenceTally::default();

    tally.record_summon(1_717_010_000); // recorded first
    tally.record_summon(1_717_000_000); // recorded second (earlier timestamp)

    assert_eq!(
        tally.first_ts, 1_717_010_000,
        "first_ts should be the first recorded ts, not the min"
    );
    assert_eq!(
        tally.last_ts, 1_717_000_000,
        "last_ts should be the last recorded ts"
    );
}

/// AC2: Tally count accumulates additively.
#[test]
fn digest_ac2_count_accumulates() {
    let mut tally = PresenceTally::default();

    for i in 0u64..10 {
        tally.record_summon(1_717_000_000 + i * 300);
    }

    assert_eq!(tally.interaction_count, 10, "expected count=10");
}
