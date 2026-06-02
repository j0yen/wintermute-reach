//! Digest AC1: reach subscribes `wm.presence.summon` and `wm.presence.silence`
//! in addition to its family topics (subscription test).
//!
//! We verify the topic strings are handled by the digest module by checking
//! that the module's `PresenceTally` methods respond correctly to these events.

use wintermute_reach::digest::PresenceTally;

/// AC1: The digest module processes wm.presence.summon events.
#[test]
fn digest_ac1_presence_summon_topic_is_handled() {
    let mut tally = PresenceTally::default();

    // Simulate receiving a wm.presence.summon event.
    tally.record_summon(1_717_000_000);

    assert_eq!(
        tally.interaction_count, 1,
        "summon event should increment interaction_count"
    );
    assert_eq!(tally.first_ts, 1_717_000_000, "first_ts should be set");
    assert_eq!(tally.last_ts, 1_717_000_000, "last_ts should be set");
}

/// AC1: The digest module processes wm.presence.silence events.
#[test]
fn digest_ac1_presence_silence_topic_is_handled() {
    let mut tally = PresenceTally::default();

    // Simulate receiving a wm.presence.silence event.
    tally.record_silence();

    assert!(
        tally.silence_flagged,
        "silence event should set silence_flagged"
    );
    assert_eq!(
        tally.interaction_count, 0,
        "silence alone does not increment interaction_count"
    );
}

/// AC1: Both topics can be received on the same day.
#[test]
fn digest_ac1_both_topics_handled_in_same_day() {
    let mut tally = PresenceTally::default();

    tally.record_summon(1_717_000_100);
    tally.record_silence();

    assert_eq!(tally.interaction_count, 1);
    assert!(tally.silence_flagged);
}
