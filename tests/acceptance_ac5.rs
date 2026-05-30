//! AC5: A transport error yields `wm.family.ack { delivered: false }` —
//! not a panic, not a silent drop.

use wintermute_reach::config::{EmailConfig, TransportConfig};
use wintermute_reach::transport::build_transport;

/// AC5: When sendmail is a non-existent binary, DeliveryResult encodes the error.
#[test]
fn acceptance_ac5_transport_error_yields_delivered_false() {
    let cfg = wintermute_reach::Config {
        transport: TransportConfig::Email(EmailConfig {
            to: "joe@example.com".to_string(),
            from: "wintermute@localhost".to_string(),
            // Point at a non-existent binary.
            sendmail: "/tmp/wm-reach-no-such-sendmail-binary-12345".to_string(),
            smtp_host: None,
        }),
        from: "wintermute".to_string(),
    };

    let transport = build_transport(&cfg).expect("build transport");
    let result = transport
        .deliver("[test]", "test body")
        .expect("deliver should not panic even on transport error");

    assert!(
        !result.delivered,
        "expected delivered=false on transport error, got: {result:?}"
    );
    assert_eq!(result.transport, "email");
    assert!(
        result.error.is_some(),
        "expected error field to be set: {result:?}"
    );
    let error_msg = result.error.expect("error");
    assert!(
        !error_msg.is_empty(),
        "error message should not be empty"
    );
}

/// AC5b: A sendmail that exits non-zero also produces delivered=false.
#[test]
fn acceptance_ac5b_sendmail_exit_nonzero_yields_delivered_false() {
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    let mut script = NamedTempFile::new().expect("script tempfile");
    write!(script, "#!/bin/sh\nexit 1\n").expect("write script");
    {
        use std::os::unix::fs::PermissionsExt as _;
        let meta = std::fs::metadata(script.path()).expect("meta");
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(script.path(), perms).expect("chmod");
    }

    let cfg = wintermute_reach::Config {
        transport: TransportConfig::Email(EmailConfig {
            to: "joe@example.com".to_string(),
            from: "wintermute@localhost".to_string(),
            sendmail: script.path().to_str().expect("path").to_string(),
            smtp_host: None,
        }),
        from: "wintermute".to_string(),
    };

    let transport = build_transport(&cfg).expect("build transport");
    let result = transport
        .deliver("[test]", "body")
        .expect("no panic on non-zero exit");

    assert!(
        !result.delivered,
        "exit-1 sendmail should yield delivered=false: {result:?}"
    );
    assert!(result.error.is_some(), "error should be set: {result:?}");
}
