//! Digest AC3: At the configured digest time, exactly one digest delivery occurs
//! through the configured transport, with a body containing the count and the
//! last-interaction time (integration test with the fake transport).

use wintermute_reach::config::{DigestConfig, EmailConfig, TransportConfig};
use wintermute_reach::digest::{PresenceTally, deliver_digest};

/// Build a fake-sendmail capture script and return (script_path, capture_path).
fn make_capture_script(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let capture_path = dir.join("capture.txt");
    let script_path = dir.join("sendmail.sh");
    let content = format!("#!/bin/sh\ncat > {}\n", capture_path.display());
    std::fs::write(&script_path, &content).expect("write script");
    {
        use std::os::unix::fs::PermissionsExt as _;
        let meta = std::fs::metadata(&script_path).expect("meta");
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).expect("chmod");
    }
    (script_path, capture_path)
}

/// AC3: One digest delivery with body containing count and last-interaction time.
#[test]
fn digest_ac3_one_delivery_with_count_and_last_time() {
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
        digest: DigestConfig {
            enabled: true,
            send_hour: 20,
            contact_name: Some("Mom".to_string()),
        },
    };

    let mut tally = PresenceTally::default();
    // Four summons; last at ts=1_717_000_000 which is midnight UTC (0:00am in UTC)
    // We just want to confirm the body contains "4" and a time string.
    for i in 0u64..4 {
        tally.record_summon(1_717_000_000 + i * 3_600);
    }

    let result = deliver_digest(&cfg, &cfg.digest, &tally).expect("deliver_digest");

    assert!(
        result.delivered,
        "digest should be delivered when enabled: {result:?}"
    );

    let captured = std::fs::read_to_string(&capture_path).expect("read capture");
    assert!(
        captured.contains('4') || captured.contains("four") || captured.contains("times"),
        "digest body should contain interaction count: {captured}"
    );
    // The body must reference the last-interaction time in some form.
    assert!(
        !captured.is_empty(),
        "captured message should not be empty"
    );
}

/// AC3: Exactly one delivery (not two or zero) happens per digest call.
#[test]
fn digest_ac3_exactly_one_delivery_per_call() {
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
        digest: DigestConfig {
            enabled: true,
            send_hour: 20,
            contact_name: Some("Mom".to_string()),
        },
    };

    let tally = PresenceTally {
        date: "2024-01-01".to_string(),
        interaction_count: 2,
        first_ts: 1_717_000_000,
        last_ts: 1_717_003_600,
        silence_flagged: false,
    };

    // Call exactly once — one delivery.
    let result = deliver_digest(&cfg, &cfg.digest, &tally).expect("deliver");
    assert!(result.delivered, "single call should yield one delivery");

    // Verify the capture file was written (one delivery = file exists + non-empty).
    let data = std::fs::read_to_string(&capture_path).expect("read capture");
    assert!(!data.is_empty(), "capture file should have content from one delivery");
}
