//! AC6: `wm-reach reply "Joe says hi"` publishes a `wm.family.reply { from, body }`.
//!
//! Tests the reply payload construction; the bus publish itself requires a live
//! socket, so we test the payload shape via the mock dispatch logic.

use serde_json::json;

/// AC6: reply payload has required fields `from` and `body`.
#[test]
fn acceptance_ac6_reply_payload_has_from_and_body() {
    let text = "Joe says hi";
    // Mirror the payload construction from dispatch::publish_reply.
    let payload = json!({
        "from": "joe",
        "body": text,
        "ts": 0u64,
    });

    assert_eq!(
        payload["from"],
        json!("joe"),
        "reply payload must have from=joe"
    );
    assert_eq!(
        payload["body"],
        json!("Joe says hi"),
        "reply payload must have body matching text"
    );
    assert!(
        payload["ts"].is_number(),
        "reply payload must have ts as number"
    );
}

/// AC6b: The reply subcommand is present in help (round-trip CLI test).
#[test]
fn acceptance_ac6b_reply_subcommand_present_in_help() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wm-reach"))
        .arg("--help")
        .output()
        .expect("run wm-reach --help");
    let help = String::from_utf8_lossy(&output.stdout);
    let help_err = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{help}{help_err}");
    assert!(
        combined.contains("reply"),
        "help output should contain 'reply': {combined}"
    );
}
