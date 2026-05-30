//! AC2: Email transport, with `WM_REACH_SENDMAIL` pointing at a capture script,
//! produces a message containing the family body.

use wintermute_reach::config::{EmailConfig, TransportConfig};
use wintermute_reach::transport::build_transport;

/// Write a capture shell script to `dir/script.sh`, make it executable,
/// and return the path. The capture destination is `dir/capture.txt`.
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

/// AC2: Fake sendmail receives message body.
#[test]
fn acceptance_ac2_email_delivers_body_to_sendmail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (script_path, capture_path) = make_capture_script(dir.path());

    let cfg = wintermute_reach::Config {
        transport: TransportConfig::Email(EmailConfig {
            to: "joe@example.com".to_string(),
            from: "wintermute@localhost".to_string(),
            sendmail: script_path.to_str().expect("path str").to_string(),
            smtp_host: None,
        }),
        from: "wintermute".to_string(),
    };

    let transport = build_transport(&cfg).expect("build transport");
    let result = transport
        .deliver("[test subject]", "heating broken")
        .expect("deliver");

    assert!(result.delivered, "expected delivered=true, got: {result:?}");
    assert_eq!(result.transport, "email");

    let captured = std::fs::read_to_string(&capture_path).expect("read capture");
    assert!(
        captured.contains("heating broken"),
        "captured message missing body: {captured}"
    );
}
