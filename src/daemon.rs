//! Daemon mode: subscribe to `wm.family.*` and `wm.presence.*`, dispatch deliveries.
//!
//! ## Subscription strategy
//!
//! The daemon subscribes to:
//! - `wm.family` (prefix match) — catches `wm.family.message` and `wm.family.distress`.
//!   Uses a two-priority channel: distress events are placed in a high-priority queue
//!   and delivered synchronously before any pending normal messages.
//! - `wm.presence` (prefix match) — catches `wm.presence.silence` to trigger the
//!   silence nudge (when `SilenceNudgeConfig::enabled = true`).
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
use crate::silence_nudge;

/// Priority levels for inbound events.
#[derive(Debug)]
enum EventKind {
    Distress { body: String },
    Message { body: String },
    /// A `wm.presence.silence` event — carry the window key for debounce.
    PresenceSilence { window_key: String },
}

/// Run the daemon subscribe loop.
///
/// Subscribes to `wm.family` and `wm.presence`, dispatches distress ahead of
/// normal messages, fires the silence nudge on `wm.presence.silence`, and
/// publishes `wm.family.ack` after each family delivery.
///
/// # Errors
///
/// Returns `Err` on unrecoverable bus errors.
pub async fn run(sock: &Path, cfg: &Config, state_dir: &Path) -> Result<()> {
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

    // Dispatch loop: drain distress first, then normal/presence.
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
                match ev {
                    Some(EventKind::Message { body }) => {
                        if let Err(e) = dispatch::handle_message(sock, cfg, &body, &session_id).await {
                            eprintln!("{{\"level\":\"error\",\"action\":\"message_ack\",\"err\":\"{e}\"}}");
                        }
                    }
                    Some(EventKind::PresenceSilence { window_key }) => {
                        match silence_nudge::maybe_deliver_nudge(
                            cfg,
                            &cfg.silence_nudge,
                            state_dir,
                            &window_key,
                        ) {
                            Ok(true) => {
                                eprintln!(
                                    "{{\"level\":\"info\",\"action\":\"silence_nudge\",\"window\":\"{window_key}\"}}"
                                );
                            }
                            Ok(false) => {} // disabled or already nudged
                            Err(e) => {
                                eprintln!(
                                    "{{\"level\":\"error\",\"action\":\"silence_nudge\",\"err\":\"{e}\"}}"
                                );
                            }
                        }
                    }
                    _ => break,
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

    client
        .subscribe("wm.presence")
        .await
        .context("subscribe wm.presence")?;

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
        } else if topic == "wm.presence.silence" {
            // Extract the window key from the event data; fall back to body text.
            let window_key = event
                .data
                .get("window")
                .and_then(|v| v.as_str())
                .map_or_else(|| body.clone(), str::to_string);
            let _ = normal_tx
                .send(EventKind::PresenceSilence { window_key })
                .await;
        }
        // wm.family.ack and wm.family.reply are filtered by self-emitted check
        // or ignored (we don't act on acks we receive from others).
        // wm.presence.summon is consumed by the digest path (not this daemon).
    }

    Ok(())
}

/// Extract the `body` field from an event payload, falling back to the full JSON.
fn extract_body(data: &serde_json::Value) -> String {
    data.get("body")
        .and_then(|v| v.as_str())
        .map_or_else(|| data.to_string(), str::to_string)
}
