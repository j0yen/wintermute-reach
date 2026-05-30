//! Daemon mode: subscribe to `wm.family.*` and dispatch deliveries.
//!
//! ## Subscription strategy
//!
//! The daemon subscribes to `wm.family` (prefix match) which catches both
//! `wm.family.message` and `wm.family.distress`. It uses a two-priority
//! channel: distress events are placed in a high-priority queue and delivered
//! synchronously before any pending normal messages.
//!
//! ## Self-emitted-topic filter
//!
//! The daemon's own `wm.family.ack` and `wm.family.reply` publishes come back
//! through the subscription if they match `wm.family`. The event loop filters
//! these by checking `event.from == own_session_id`. Since the daemon announces
//! with `wm-reach-daemon-<pid>`, any event with `from` equal to that `session_id`
//! is skipped without processing.

#![allow(
    unreachable_pub,
    clippy::future_not_send,
    clippy::too_many_lines,
    clippy::print_stderr,
)]

use anyhow::{Context as _, Result};
use std::path::Path;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::dispatch;

/// Priority levels for inbound events.
#[derive(Debug)]
enum EventKind {
    Distress { body: String },
    Message { body: String },
}

/// Run the daemon subscribe loop.
///
/// Subscribes to `wm.family`, dispatches distress ahead of normal messages,
/// and publishes `wm.family.ack` after each delivery.
///
/// # Errors
///
/// Returns `Err` on unrecoverable bus errors.
pub async fn run(sock: &Path, cfg: &Config) -> Result<()> {
    let pid = std::process::id();
    let session_id = format!("wm-reach-daemon-{pid}");

    // Two-channel priority queue: distress is high-priority.
    let (distress_tx, mut distress_rx) = mpsc::channel::<EventKind>(64);
    let (normal_tx, mut normal_rx) = mpsc::channel::<EventKind>(256);

    let sock_buf = sock.to_path_buf();
    let session_id_clone = session_id.clone();

    // Subscriber task: connects, announces, subscribes, and routes events.
    let sub_task = tokio::spawn(async move {
        subscribe_loop(
            &sock_buf,
            &session_id_clone,
            pid,
            distress_tx,
            normal_tx,
        )
        .await
    });

    // Dispatch loop: drain distress first, then normal.
    loop {
        tokio::select! {
            biased; // distress checked first
            ev = distress_rx.recv() => {
                let Some(EventKind::Distress { body }) = ev else { break; };
                if let Err(e) = dispatch::handle_distress(sock, cfg, &body, &session_id).await {
                    eprintln!("{{\"level\":\"error\",\"action\":\"distress_ack\",\"err\":\"{e}\"}}");
                }
            }
            ev = normal_rx.recv() => {
                let Some(EventKind::Message { body }) = ev else { break; };
                if let Err(e) = dispatch::handle_message(sock, cfg, &body, &session_id).await {
                    eprintln!("{{\"level\":\"error\",\"action\":\"message_ack\",\"err\":\"{e}\"}}");
                }
            }
        }
    }

    sub_task
        .await
        .context("subscriber task panicked")?
        .context("subscriber task error")
}

/// Inner subscribe loop that feeds the priority channels.
async fn subscribe_loop(
    sock: &Path,
    session_id: &str,
    pid: u32,
    distress_tx: mpsc::Sender<EventKind>,
    normal_tx: mpsc::Sender<EventKind>,
) -> Result<()> {
    let mut client = agorabus::Client::connect(sock)
        .await
        .context("connecting to bus")?;

    client
        .announce(session_id, pid, "/", "wm-reach daemon")
        .await
        .context("announce")?;

    client
        .subscribe("wm.family")
        .await
        .context("subscribe wm.family")?;

    loop {
        let Some(event) = client.next_event().await? else {
            break;
        };

        // Self-emitted-topic filter: skip own publishes.
        if event.from == session_id {
            continue;
        }

        let topic = event.topic.as_str();
        let body = extract_body(&event.data);

        if topic == "wm.family.distress" {
            let _ = distress_tx.send(EventKind::Distress { body }).await;
        } else if topic == "wm.family.message" {
            let _ = normal_tx.send(EventKind::Message { body }).await;
        }
        // wm.family.ack and wm.family.reply are filtered by self-emitted check
        // or ignored (we don't act on acks we receive from others).
    }

    Ok(())
}

/// Extract the `body` field from an event payload, falling back to the full JSON.
fn extract_body(data: &serde_json::Value) -> String {
    data.get("body")
        .and_then(|v| v.as_str())
        .map_or_else(|| data.to_string(), str::to_string)
}
