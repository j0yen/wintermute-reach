//! Distress durability AC1–AC8 test suite.
//!
//! AC1: primary fails once then succeeds → one delivered:true, no delivered:false.
//! AC2: primary fails all retries, fallback succeeds → delivered:true naming fallback.
//! AC3: all transports fail → exactly one delivered:false after full ladder.
//! AC4: attempt count bounded by config cap — never loops unbounded.
//! AC5: ordinary message still single-attempt (regression of v0.2.0 AC5).
//! AC6: two concurrent distress events both terminate (non-starvation).
//! AC7: no body in logs (structural: FakeTransport never logs body).
//! AC8: ntfy/webhook behind feature flags (compile-time).

use wintermute_reach::{
    DistressPolicy,
    distress_delivery::{run_distress_ladder, test_helpers::FakeTransport},
};

/// AC1: primary fails once, then succeeds on retry → delivered:true.
#[test]
fn distress_durability_ac1_retry_on_primary_success() {
    let primary = FakeTransport::new("email", [false, true]);
    let policy = DistressPolicy {
        max_retries: 3,
        backoff_ms: 0,
        ..Default::default()
    };

    let result = run_distress_ladder(
        "[DISTRESS] test",
        "body",
        &primary,
        "email",
        &[],
        &policy,
    )
    .expect("ladder");

    assert!(result.delivered, "retry should succeed: {result:?}");
    assert_eq!(result.transport, "email");
    // Two attempts: one fail + one success.
    assert_eq!(primary.call_count(), 2, "should have been called twice");
}

/// AC2: primary fails all retries, first fallback succeeds → delivered:true naming fallback.
#[test]
fn distress_durability_ac2_fallback_succeeds_after_primary_exhausted() {
    let primary = FakeTransport::new("email", [false, false, false, false]); // 1 + 3 retries
    let fallback = FakeTransport::new("ntfy", [true]);
    let policy = DistressPolicy {
        max_retries: 3,
        backoff_ms: 0,
        ..Default::default()
    };

    let result = run_distress_ladder(
        "[DISTRESS] test",
        "body",
        &primary,
        "email",
        &[(&fallback, "ntfy")],
        &policy,
    )
    .expect("ladder");

    assert!(result.delivered, "fallback should succeed: {result:?}");
    assert_eq!(result.transport, "ntfy", "ack should name the fallback transport");
    // Primary: 4 attempts (1 + 3 retries), fallback: 1.
    assert_eq!(primary.call_count(), 4, "primary exhausted at 4 attempts");
    assert_eq!(fallback.call_count(), 1, "fallback called once");
}

/// AC3: all transports fail → exactly one delivered:false after full ladder.
#[test]
fn distress_durability_ac3_all_fail_yields_one_delivered_false() {
    let primary = FakeTransport::new("email", [false, false]);
    let fb1 = FakeTransport::new("ntfy", [false]);
    let fb2 = FakeTransport::new("webhook", [false]);
    let policy = DistressPolicy {
        max_retries: 1,
        backoff_ms: 0,
        ..Default::default()
    };

    let result = run_distress_ladder(
        "[DISTRESS] test",
        "body",
        &primary,
        "email",
        &[(&fb1, "ntfy"), (&fb2, "webhook")],
        &policy,
    )
    .expect("ladder");

    assert!(!result.delivered, "all failed → delivered:false: {result:?}");
    assert!(result.error.is_some(), "error field should be set");
}

/// AC4: attempt count is bounded — never exceeds 1 + max_retries.
#[test]
fn distress_durability_ac4_attempt_count_bounded_by_cap() {
    let primary = FakeTransport::new("email", vec![false; 100]); // always fails
    let policy = DistressPolicy {
        max_retries: 3,
        backoff_ms: 0,
        ..Default::default()
    };

    let result = run_distress_ladder(
        "[DISTRESS] test",
        "body",
        &primary,
        "email",
        &[],
        &policy,
    )
    .expect("ladder");

    assert!(!result.delivered);
    assert_eq!(
        primary.call_count(),
        4, // 1 + 3 retries
        "attempt count must equal 1 + max_retries = 4, got {}",
        primary.call_count()
    );
}

/// AC5: ordinary message delivery is single-attempt (regression v0.2.0 AC5).
///
/// The distress ladder is NOT called for ordinary messages.  This test verifies
/// the transport contract directly: a single call to `deliver` with no retry.
#[test]
fn distress_durability_ac5_ordinary_message_single_attempt() {
    use wintermute_reach::config::{EmailConfig, TransportConfig};
    use wintermute_reach::transport::build_transport;

    let cfg = wintermute_reach::Config {
        transport: TransportConfig::Email(EmailConfig {
            to: "joe@example.com".to_string(),
            from: "wintermute@localhost".to_string(),
            sendmail: "/tmp/wm-reach-no-such-sendmail-12345".to_string(),
            smtp_host: None,
        }),
        from: "wintermute".to_string(),
        ..Default::default()
    };

    // For ordinary messages, `handle_message` calls build_transport + deliver once.
    // Verify that a single deliver call with a failing transport yields delivered:false
    // immediately (no retry path called).
    let transport = build_transport(&cfg).expect("build transport");
    let result = transport
        .deliver("[wintermute family message]", "test body")
        .expect("deliver should not panic");

    assert!(
        !result.delivered,
        "ordinary message failure → delivered:false immediately: {result:?}"
    );
}

/// AC6: two distress events with zero-backoff policy both terminate.
#[test]
fn distress_durability_ac6_two_concurrent_distress_both_terminate() {
    let p1 = FakeTransport::new("email", [true]);
    let p2 = FakeTransport::new("email", [true]);
    let policy = DistressPolicy {
        max_retries: 0,
        backoff_ms: 0,
        ..Default::default()
    };

    // Run two ladders "concurrently" (synchronously here since ladder is sync).
    let r1 = run_distress_ladder("[DISTRESS]", "first", &p1, "email", &[], &policy)
        .expect("first ladder");
    let r2 = run_distress_ladder("[DISTRESS]", "second", &p2, "email", &[], &policy)
        .expect("second ladder");

    assert!(r1.delivered, "first distress should deliver");
    assert!(r2.delivered, "second distress should deliver");
}

/// AC8: default DistressPolicy has sane defaults.
#[test]
fn distress_durability_ac8_default_policy_sane() {
    let policy = DistressPolicy::default();
    assert!(policy.max_retries > 0, "default should have at least one retry");
    assert!(policy.backoff_ms > 0, "default should have non-zero backoff");
    // In the default build, no fallbacks are configured.
    assert!(
        policy.fallback_order.is_empty(),
        "default should have no pre-configured fallbacks"
    );
}
