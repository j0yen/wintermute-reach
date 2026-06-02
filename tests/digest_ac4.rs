//! Digest AC4: A day with zero summons produces a "quiet day" digest body.

use wintermute_reach::config::DigestConfig;
use wintermute_reach::digest::PresenceTally;

/// AC4: Zero summons → "quiet day" body.
#[test]
fn digest_ac4_zero_summons_produces_quiet_day_body() {
    let tally = PresenceTally::default();
    let digest_cfg = DigestConfig {
        enabled: true,
        send_hour: 20,
        contact_name: Some("Mom".to_string()),
    };

    let body = tally.format_digest_body(
        digest_cfg.contact_name.as_deref().unwrap_or("Mom"),
    );

    assert!(
        body.to_lowercase().contains("quiet"),
        "zero-summon body should mention 'quiet': {body}"
    );
}

/// AC4: Body includes the contact name.
#[test]
fn digest_ac4_quiet_body_includes_contact_name() {
    let tally = PresenceTally::default();
    let body = tally.format_digest_body("Grandma");

    assert!(
        body.contains("Grandma"),
        "quiet body should include the contact name: {body}"
    );
}

/// AC4: A tally with summons does NOT produce a "quiet day" body.
#[test]
fn digest_ac4_nonzero_summons_not_quiet() {
    let mut tally = PresenceTally::default();
    tally.record_summon(1_717_000_000);

    let body = tally.format_digest_body("Mom");

    assert!(
        !body.to_lowercase().contains("quiet"),
        "non-zero tally should not say 'quiet': {body}"
    );
}
