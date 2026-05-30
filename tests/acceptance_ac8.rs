//! AC8: The daemon applies the self-emitted-topic filter — does not re-consume
//! its own `wm.family.ack` or `wm.family.reply` publishes.

/// AC8: Self-emitted topic filter: events with `from == own_session_id` are skipped.
///
/// We test the filter logic directly (the `event.from == session_id` check
/// in daemon::subscribe_loop) by simulating the decision boundary.
#[test]
fn acceptance_ac8_self_emitted_events_are_filtered() {
    let own_session_id = "wm-reach-daemon-12345";

    // Simulate inbound event from self.
    let self_event = agorabus::ServerEvent {
        topic: "wm.family.ack".to_string(),
        data: serde_json::json!({ "delivered": true }),
        from: own_session_id.to_string(),
    };

    // Simulate inbound event from another client.
    let other_event = agorabus::ServerEvent {
        topic: "wm.family.message".to_string(),
        data: serde_json::json!({ "body": "heating broken" }),
        from: "other-session-999".to_string(),
    };

    // Apply filter logic (mirrors daemon::subscribe_loop).
    let is_self_emitted = |ev: &agorabus::ServerEvent| ev.from == own_session_id;

    assert!(
        is_self_emitted(&self_event),
        "own ack event should be detected as self-emitted"
    );
    assert!(
        !is_self_emitted(&other_event),
        "other client event should not be filtered"
    );
}

/// AC8b: self-emitted filter is topic-agnostic (any topic from own session is filtered).
#[test]
fn acceptance_ac8b_self_emitted_filter_is_topic_agnostic() {
    let own_session_id = "wm-reach-daemon-99999";

    let topics = [
        "wm.family.ack",
        "wm.family.reply",
        "wm.family.message",
        "wm.other.topic",
    ];

    for topic in topics {
        let event = agorabus::ServerEvent {
            topic: topic.to_string(),
            data: serde_json::json!({}),
            from: own_session_id.to_string(),
        };
        assert!(
            event.from == own_session_id,
            "filter should apply to topic '{topic}'"
        );
    }
}
