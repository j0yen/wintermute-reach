//! Distress delivery ladder — bounded retry + multi-transport escalation.
//!
//! # Design invariants
//!
//! - **Distress-only scope** — ordinary `wm.family.message` deliveries are
//!   single-attempt (v0.2.0 AC5 unchanged).  Only `wm.family.distress` runs
//!   the ladder.
//! - **Non-blocking** — the ladder is intended to run in a `tokio::spawn` task
//!   off the daemon select loop so a second distress is not starved.
//! - **Bounded** — `DistressPolicy::max_retries` caps the attempt count;
//!   `DistressPolicy::backoff_ms` caps the inter-attempt pause.
//! - **Observability** — each attempt logs transport + outcome (no body).
//!   The final ack names the transport that succeeded (or `delivered: false`
//!   after the whole ladder is exhausted).
//! - **No body in logs** — only transport id and outcome are logged.

#![allow(clippy::print_stderr)]

use anyhow::Result;

use crate::config::DistressPolicy;
use crate::transport::{DeliveryResult, Transport};

// --------------------------------------------------------------------------
// Ladder runner
// --------------------------------------------------------------------------

/// Run the distress delivery ladder synchronously (no async).
///
/// Attempts the primary transport up to `1 + policy.max_retries` times with
/// `policy.backoff_ms` pauses between attempts.  On continued failure, tries
/// each fallback in order (one attempt each).  Returns the first success or
/// the final failure result after the entire ladder is exhausted.
///
/// **Body is never logged** — only transport id and attempt number appear in logs.
///
/// # Errors
///
/// Returns `Err` only if a transport's `deliver` call itself errors (which the
/// `Transport` contract says should not happen; errors are encoded in the result).
pub fn run_distress_ladder(
    subject: &str,
    body: &str,
    primary: &dyn Transport,
    primary_name: &str,
    fallbacks: &[(&dyn Transport, &str)],
    policy: &DistressPolicy,
) -> Result<DeliveryResult> {
    // --- Primary transport with retries ---
    let max_attempts = 1_u32.saturating_add(policy.max_retries);
    let mut last_result: Option<DeliveryResult> = None;

    for attempt in 0..max_attempts {
        let result = primary.deliver(subject, body)?;

        eprintln!(
            "{{\"level\":\"info\",\"action\":\"distress_attempt\",\"transport\":\"{}\",\"attempt\":{},\"delivered\":{}}}",
            primary_name, attempt, result.delivered
        );

        if result.delivered {
            return Ok(result);
        }

        last_result = Some(result);

        // Sleep between retries but not after the last attempt.
        if attempt + 1 < max_attempts && policy.backoff_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(policy.backoff_ms));
        }
    }

    // --- Fallback transports (one attempt each) ---
    for (fb_transport, fb_name) in fallbacks {
        let result = fb_transport.deliver(subject, body)?;

        eprintln!(
            "{{\"level\":\"info\",\"action\":\"distress_fallback\",\"transport\":\"{}\",\"delivered\":{}}}",
            fb_name, result.delivered
        );

        if result.delivered {
            return Ok(result);
        }

        last_result = Some(result);
    }

    // Whole ladder exhausted — return the last failure.
    Ok(last_result.unwrap_or_else(|| DeliveryResult {
        delivered: false,
        transport: primary_name.to_string(),
        reference: None,
        error: Some("distress ladder exhausted with no attempts".to_string()),
    }))
}

// --------------------------------------------------------------------------
// FakeTransport for tests
//
// Not gated by #[cfg(test)] because integration tests in tests/ link against
// the non-test library and cannot access cfg(test) items.  `#[doc(hidden)]`
// keeps it out of the public docs.
// --------------------------------------------------------------------------

/// Test helpers for the distress delivery ladder.
///
/// Exported for integration tests; not intended for production use.
#[doc(hidden)]
pub mod test_helpers {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A fake transport that returns a pre-configured sequence of outcomes.
    ///
    /// Used in integration tests to drive the distress ladder through
    /// fail/succeed scenarios without network access.
    pub struct FakeTransport {
        name: String,
        outcomes: Arc<Mutex<std::collections::VecDeque<bool>>>,
        call_count: Arc<Mutex<u32>>,
    }

    impl FakeTransport {
        /// Construct with a fixed sequence of `delivered` outcomes.
        #[must_use]
        pub fn new(name: impl Into<String>, outcomes: impl IntoIterator<Item = bool>) -> Self {
            Self {
                name: name.into(),
                outcomes: Arc::new(Mutex::new(outcomes.into_iter().collect())),
                call_count: Arc::new(Mutex::new(0)),
            }
        }

        /// Number of times `deliver` was called.
        ///
        /// Returns 0 if the mutex is poisoned (should never happen in tests).
        #[must_use]
        pub fn call_count(&self) -> u32 {
            self.call_count
                .lock()
                .map_or(0, |g| *g)
        }
    }

    impl Transport for FakeTransport {
        fn deliver(&self, _subject: &str, _body: &str) -> anyhow::Result<DeliveryResult> {
            let mut count = self
                .call_count
                .lock()
                .map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
            *count += 1;

            let delivered = self
                .outcomes
                .lock()
                .map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?
                .pop_front()
                .unwrap_or(false);

            Ok(DeliveryResult {
                delivered,
                transport: self.name.clone(),
                reference: None,
                error: if delivered {
                    None
                } else {
                    Some("fake failure".to_string())
                },
            })
        }
    }
}
