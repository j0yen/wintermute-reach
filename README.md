# wintermute-reach

Off-device transport boundary for the [wintermute](https://github.com/j0yen/wintermute) family-intents system.

## Overview

`wintermute-reach` is a long-running agorabus daemon that crosses the local/off-device boundary: it subscribes to `wm.family.message` and `wm.family.distress` events, delivers them to jsy through a real transport (email by default; ntfy and webhook behind Cargo features), and acks delivery back onto the bus so the dialog FSM can say "I let Joe know."

Distress events bypass any normal message queue and are delivered synchronously with highest priority. A v1 inbound-reply stub (`wm-reach reply`) lets the full dialog loop be tested end-to-end without an external inbound channel.

## Acceptance Criteria

1. `wm-reach --help` lists `daemon`, `send`, `reply`, `test-transport`.
2. Email transport, with `WM_REACH_SENDMAIL` pointing at a capture script, produces a message containing the family body.
3. A published `wm.family.message { to: "joe", body: "..." }` results in one transport delivery and one `wm.family.ack { delivered: true, transport: "email" }` on the bus.
4. A published `wm.family.distress` is delivered ahead of a `wm.family.message` that was queued first.
5. A transport error yields `wm.family.ack { delivered: false }` — not a panic, not a silent drop.
6. `wm-reach reply "Joe says hi"` publishes a `wm.family.reply { from, body }` that a bus subscriber receives.
7. ntfy and webhook backends compile behind their Cargo features and are excluded from the default build.
8. The daemon applies the self-emitted-topic filter (does not re-consume its own ack/reply publishes).
9. systemd unit `wm-reach.service` installs pointing at `.local/bin/wm-reach`.
10. No secret is logged; config is read from `/etc/wintermute/conf.d/`, never hard-coded.
11. `cargo test` green; `cargo clippy` clean; release gate receipts produced per autobuilder.

## Install

```sh
cargo build --release
install -Dm755 target/release/wm-reach ~/.local/bin/wm-reach
cp dist/wm-reach.service ~/.config/systemd/user/
systemctl --user enable --now wm-reach.service
```

## Configuration

Config is loaded from `/etc/wintermute/conf.d/reach.json` (or `$WM_REACH_CONF_DIR`). If no config file is present, environment variables are used:

- `WM_REACH_TO` — recipient email address (default: `jyen.tech@gmail.com`)
- `WM_REACH_FROM` — sender address (default: `wintermute@localhost`)
- `WM_REACH_SENDMAIL` — path to sendmail binary (default: `/usr/sbin/sendmail`)
- `AGORABUS_SOCK` — bus socket path (default: `~/.cache/agorabus/sock`)

Example `reach.json`:
```json
{
  "transport": {
    "kind": "email",
    "to": "jyen.tech@gmail.com",
    "from": "wintermute@localhost",
    "sendmail": "/usr/sbin/sendmail"
  }
}
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
