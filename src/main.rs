//! `wm-reach` — off-device transport daemon for the wintermute family-intents system.
//!
//! # Subcommands
//!
//! - `daemon`         — run the subscribe loop (long-lived service)
//! - `send`           — one-shot manual delivery (testing)
//! - `reply`          — publish a `wm.family.reply` (v1 inbound stub)
//! - `test-transport` — dry-run the configured backend

#![deny(unsafe_code)]

use anyhow::Result;
use clap::{Parser, Subcommand};

mod config;
mod daemon;
mod dispatch;
mod transport;

pub use config::Config;

/// Off-device transport boundary for wintermute family-intents.
#[derive(Parser, Debug)]
#[command(name = "wm-reach", version, about)]
pub struct Cli {
    /// Path to config directory (default: /etc/wintermute/conf.d)
    #[arg(long, env = "WM_REACH_CONF_DIR", default_value = "/etc/wintermute/conf.d")]
    pub conf_dir: std::path::PathBuf,

    /// Path to the agorabus socket.
    #[arg(long, env = "AGORABUS_SOCK")]
    pub bus_sock: Option<std::path::PathBuf>,

    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Subcommands for `wm-reach`.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the subscribe loop (systemd wm-reach.service).
    Daemon,
    /// Manual one-shot delivery (testing).
    Send {
        /// Recipient name (e.g. "joe")
        #[arg(long, default_value = "joe")]
        to: String,
        /// Message body
        #[arg(long)]
        body: String,
    },
    /// Publish a wm.family.reply (v1 inbound stub).
    Reply {
        /// Reply text to publish
        text: String,
    },
    /// Dry-run the configured backend, print result.
    TestTransport,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let sock = cli
        .bus_sock
        .unwrap_or_else(agorabus::default_socket_path);
    let cfg = config::Config::load(&cli.conf_dir)?;

    match cli.command {
        Command::Daemon => {
            daemon::run(&sock, &cfg).await?;
        }
        Command::Send { to, body } => {
            dispatch::send_one(&sock, &cfg, &to, &body).await?;
        }
        Command::Reply { text } => {
            dispatch::publish_reply(&sock, &text).await?;
        }
        Command::TestTransport => {
            let result = transport::test_transport(&cfg)?;
            // Deliberate: test-transport output goes to stderr (diagnostic subcommand).
            #[allow(clippy::print_stderr)]
            {
                eprintln!("{}", serde_json::to_string_pretty(&result)?);
            }
        }
    }
    Ok(())
}
