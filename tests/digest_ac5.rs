//! Digest AC5: A flagged `wm.presence.silence` is reflected in that day's
//! digest line and does NOT trigger an immediate separate delivery (it waits
//! for the digest — only distress is instant).

use wintermute_reach::digest::PresenceTally;

/// AC5: Silence flag appears in digest body.
#[test]
fn digest_ac5_silence_reflected_in_digest_body() {
    let mut tally = PresenceTally::default();
    tally.record_summon(1_717_000_000);
    tally.record_silence();

    let body = tally.format_digest_body("Mom");

    assert!(
        body.to_lowercase().contains("silence")
            || body.to_lowercase().contains("flagged")
            || body.to_lowercase().contains("window")
            || body.to_lowercase().contains("note"),
        "silence flag should be reflected in digest body: {body}"
    );
}

/// AC5: Silence flag does NOT appear when silence was not recorded.
#[test]
fn digest_ac5_no_silence_no_flag_in_body() {
    let mut tally = PresenceTally::default();
    tally.record_summon(1_717_000_000);
    // No record_silence() call.

    let body = tally.format_digest_body("Mom");

    assert!(
        !body.to_lowercase().contains("silence") && !body.to_lowercase().contains("flagged"),
        "body should not mention silence when not flagged: {body}"
    );
}

/// AC5: Silence does not increase interaction_count (delivery count unchanged).
///
/// This verifies that silence does not pretend to be a summon.
#[test]
fn digest_ac5_silence_does_not_increment_count() {
    let mut tally = PresenceTally::default();
    tally.record_summon(1_717_000_000);
    tally.record_silence(); // Should not bump count.

    assert_eq!(
        tally.interaction_count, 1,
        "silence should not increment interaction_count"
    );
}

/// AC5: The `silence_flagged` field round-trips through the tally struct.
#[test]
fn digest_ac5_silence_flag_round_trips() {
    let mut tally = PresenceTally::default();
    assert!(!tally.silence_flagged, "starts unset");
    tally.record_silence();
    assert!(tally.silence_flagged, "set after record_silence");
    // Reset clears it.
    tally.reset("2024-02-01");
    assert!(!tally.silence_flagged, "cleared after reset");
}
