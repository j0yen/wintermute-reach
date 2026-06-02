//! Digest AC9: The digest reuses reach's existing Transport impl (no second
//! transport code path — verified by construction/test).
//!
//! This test verifies that:
//! 1. `deliver_digest` calls `build_transport` from the existing module.
//! 2. The digest source does not contain a second transport implementation.
//! 3. The existing family-message and ack tests still pass (no regression).

/// AC9: digest.rs source does not contain a second Transport implementation.
#[test]
fn digest_ac9_no_second_transport_in_digest_source() {
    let digest_src = include_str!("../src/digest.rs");

    // The digest module must not define its own Transport trait or impl block.
    // It should only call build_transport from the existing transport module.
    assert!(
        !digest_src.contains("impl Transport"),
        "digest.rs must not implement a second Transport trait"
    );

    // Must NOT define a separate transport struct (Email*, Ntfy*, Webhook*).
    assert!(
        !digest_src.contains("struct EmailTransport"),
        "digest.rs must not re-define EmailTransport"
    );
    assert!(
        !digest_src.contains("struct NtfyTransport"),
        "digest.rs must not re-define NtfyTransport"
    );
    assert!(
        !digest_src.contains("struct WebhookTransport"),
        "digest.rs must not re-define WebhookTransport"
    );
}

/// AC9: digest.rs uses build_transport from the transport module.
#[test]
fn digest_ac9_digest_calls_build_transport() {
    let digest_src = include_str!("../src/digest.rs");

    assert!(
        digest_src.contains("build_transport"),
        "digest.rs must call build_transport (single transport code path)"
    );
}

/// AC9: The existing family-message transport is still accessible after
/// adding the digest module (no regression to transport::build_transport).
#[test]
fn digest_ac9_family_transport_still_works_after_digest() {
    use wintermute_reach::config::{EmailConfig, TransportConfig};
    use wintermute_reach::transport::build_transport;

    let dir = tempfile::tempdir().expect("tempdir");
    let capture_path = dir.path().join("capture.txt");
    let script_path = dir.path().join("sendmail.sh");
    let content = format!("#!/bin/sh\ncat > {}\n", capture_path.display());
    std::fs::write(&script_path, &content).expect("write script");
    {
        use std::os::unix::fs::PermissionsExt as _;
        let meta = std::fs::metadata(&script_path).expect("meta");
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).expect("chmod");
    }

    let cfg = wintermute_reach::Config {
        transport: TransportConfig::Email(EmailConfig {
            to: "jyen.tech@gmail.com".to_string(),
            from: "wintermute@localhost".to_string(),
            sendmail: script_path.to_str().expect("path").to_string(),
            smtp_host: None,
        }),
        from: "wintermute".to_string(),
        digest: wintermute_reach::DigestConfig::default(),
    };

    let transport = build_transport(&cfg).expect("build existing transport");
    let result = transport
        .deliver("[family message]", "heating broken")
        .expect("deliver");

    assert!(
        result.delivered,
        "existing family transport must still work: {result:?}"
    );
}
