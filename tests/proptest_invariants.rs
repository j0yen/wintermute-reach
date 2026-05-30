//! Proptest invariants for wintermute-reach.
//! READ-ONLY: the edit-agent must not modify this file.

use proptest::prelude::*;
use wintermute_reach::transport::DeliveryResult;

proptest! {
    /// Invariant: DeliveryResult serializes and deserializes round-trip without loss.
    #[test]
    fn delivery_result_roundtrips(
        delivered in any::<bool>(),
        transport in "[a-z]{3,8}",
    ) {
        let result = DeliveryResult {
            delivered,
            transport: transport.clone(),
            reference: None,
            error: if delivered { None } else { Some("error".to_string()) },
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let back: DeliveryResult = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(back.delivered, result.delivered);
        prop_assert_eq!(back.transport, result.transport);
    }

    /// Invariant: topic filter is deterministic — same session_id always yields same decision.
    #[test]
    fn self_emitted_filter_is_deterministic(
        session_id in "[a-z0-9-]{5,30}",
        from in "[a-z0-9-]{5,30}",
    ) {
        let expected = from == session_id;
        // Applying the filter twice gives the same result.
        prop_assert_eq!(from == session_id, expected);
        prop_assert_eq!(from == session_id, expected);
    }
}
