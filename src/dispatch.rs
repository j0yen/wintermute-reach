//! Message dispatch logic: build ack payloads, publish replies, handle priority.
//!
//! This module owns the logic for:
//! - Delivering a message via the active transport and publishing the ack
//! - Filtering self-emitted topics
//! - One-shot `send` and `reply` subcommand implementations

// Items pub for lib/test use; same module is compiled in-tree for the bin.
#![allow(unreachable_pub, clippy::print_stderr)]

use anyhow::{Context as _, Result};
use serde_json::json;
use std::path::Path;

use crate::config::Config;
use crate::transport::{DeliveryResult, build_transport};

/// Publish a `wm.family.ack` payload onto the bus.
///
/// # Errors
///
/// Returns `Err` on bus connection or publish failure.
pub async fn publish_ack(
    sock: &Path,
    result: &DeliveryResult,
    session_id: &str,
) -> Result<()> {
    let mut client = agorabus::Client::connect(sock)
        .await
        .context("connecting to bus for ack")?;
    client
        .announce(session_id, std::process::id(), "/", "wm-reach ack")
        .await
        .context("announce for ack")?;
    let payload = json!({
        "delivered": result.delivered,
        "transport": result.transport,
        "ref": result.reference,
        "error": result.error,
        "ts": unix_now_secs(),
    });
    client
        .publish("wm.family.ack", payload)
        .await
        .context("publish wm.family.ack")?;
    Ok(())
}

/// Publish a `wm.family.reply` for the `reply` subcommand (v1 inbound stub).
///
/// # Errors
///
/// Returns `Err` on bus connection or publish failure.
pub async fn publish_reply(sock: &Path, text: &str) -> Result<()> {
    let session_id = format!("wm-reach-reply-{}", std::process::id());
    let mut client = agorabus::Client::connect(sock)
        .await
        .context("connecting to bus for reply")?;
    client
        .announce(&session_id, std::process::id(), "/", "wm-reach reply")
        .await
        .context("announce for reply")?;
    let payload = json!({
        "from": "joe",
        "body": text,
        "ts": unix_now_secs(),
    });
    client
        .publish("wm.family.reply", payload)
        .await
        .context("publish wm.family.reply")?;
    Ok(())
}

/// One-shot manual delivery for the `send` subcommand.
///
/// # Errors
///
/// Returns `Err` on transport or bus failure.
pub async fn send_one(sock: &Path, cfg: &Config, to: &str, body: &str) -> Result<()> {
    // Perform transport delivery synchronously before any await.
    let result = {
        let transport = build_transport(cfg)?;
        let subject = format!("[wintermute \u{2192} {to}]");
        transport
            .deliver(&subject, body)
            .context("transport delivery")?
    };
    let session_id = format!("wm-reach-send-{}", std::process::id());
    publish_ack(sock, &result, &session_id).await?;
    if result.delivered {
        eprintln!(
            "{{\"status\":\"delivered\",\"transport\":\"{}\"}}",
            result.transport
        );
    } else {
        let err_json = result
            .error
            .as_deref()
            .map_or_else(
                || "null".to_string(),
                |e| serde_json::to_string(e).unwrap_or_else(|_| "null".to_string()),
            );
        eprintln!(
            "{{\"status\":\"failed\",\"transport\":\"{}\",\"error\":{}}}",
            result.transport, err_json
        );
    }
    Ok(())
}

/// Deliver an inbound `wm.family.message` event and publish the ack.
///
/// Called from the daemon event loop.
///
/// # Errors
///
/// Returns `Err` on ack-publish failure (transport failures are encoded in the ack).
pub async fn handle_message(
    sock: &Path,
    cfg: &Config,
    body: &str,
    session_id: &str,
) -> Result<()> {
    // Synchronous transport delivery before any await.
    let result = {
        let transport = build_transport(cfg)?;
        transport
            .deliver("[wintermute family message]", body)
            .unwrap_or_else(|e| DeliveryResult {
                delivered: false,
                transport: cfg.transport_kind().to_string(),
                reference: None,
                error: Some(format!("transport error: {e}")),
            })
    };
    publish_ack(sock, &result, session_id).await
}

/// Deliver an inbound `wm.family.distress` event and publish the ack.
///
/// Distress is delivered synchronously before normal messages; the calling
/// loop ensures ordering by invoking this *first* in its dispatch priority.
///
/// # Errors
///
/// Returns `Err` on ack-publish failure.
pub async fn handle_distress(
    sock: &Path,
    cfg: &Config,
    body: &str,
    session_id: &str,
) -> Result<()> {
    // Synchronous transport delivery before any await.
    let result = {
        let transport = build_transport(cfg)?;
        transport
            .deliver("[DISTRESS] wintermute family alert", body)
            .unwrap_or_else(|e| DeliveryResult {
                delivered: false,
                transport: cfg.transport_kind().to_string(),
                reference: None,
                error: Some(format!("transport error: {e}")),
            })
    };
    publish_ack(sock, &result, session_id).await
}

/// Current UNIX seconds (non-failing).
fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
