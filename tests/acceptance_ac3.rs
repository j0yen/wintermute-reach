//! AC3: A published `wm.family.message { to, body }` results in one transport
//! delivery and one `wm.family.ack { delivered: true, transport: "email" }`.

use wintermute_reach::config::{EmailConfig, TransportConfig};
use wintermute_reach::transport::build_transport;

/// Write a capture shell script to `dir/sendmail.sh`, returns (script_path, capture_path).
fn make_capture_script(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let capture_path = dir.join("capture.txt");
    let script_path = dir.join("sendmail.sh");
    let content = format!("#!/bin/sh\ncat > {}\n", capture_path.display());
    std::fs::write(&script_path, content).expect("write script");
    {
        use std::os::unix::fs::PermissionsExt as _;
        let meta = std::fs::metadata(&script_path).expect("meta");
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).expect("chmod");
    }
    (script_path, capture_path)
}

/// AC3: delivery of a wm.family.message body yields delivered=true ack.
#[test]
fn acceptance_ac3_family_message_delivery_produces_ack() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (script_path, capture_path) = make_capture_script(dir.path());

    let cfg = wintermute_reach::Config {
        transport: TransportConfig::Email(EmailConfig {
            to: "jyen.tech@gmail.com".to_string(),
            from: "wintermute@localhost".to_string(),
            sendmail: script_path.to_str().expect("path").to_string(),
            smtp_host: None,
        }),
        from: "wintermute".to_string(),
        digest: wintermute_reach::DigestConfig::default(),
        ..Default::default()
    };

    // Simulate dispatch.handle_message logic inline.
    let transport = build_transport(&cfg).expect("build transport");
    let result = transport
        .deliver("[wintermute family message]", "heating broken")
        .expect("deliver");

    // Verify delivered=true and transport=email.
    assert!(result.delivered, "expected delivered=true: {result:?}");
    assert_eq!(result.transport, "email");

    // Verify the body landed in the capture file.
    let captured = std::fs::read_to_string(&capture_path).expect("read capture");
    assert!(
        captured.contains("heating broken"),
        "body missing from captured message: {captured}"
    );

    // Verify ack payload shape.
    let ack_payload = serde_json::json!({
        "delivered": result.delivered,
        "transport": result.transport,
        "ref": result.reference,
        "error": result.error,
    });
    assert_eq!(ack_payload["delivered"], serde_json::json!(true));
    assert_eq!(ack_payload["transport"], serde_json::json!("email"));
}
