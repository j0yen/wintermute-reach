//! Digest AC8: With digest disabled in config, no digest is ever delivered (opt-in gate).

use wintermute_reach::config::{DigestConfig, EmailConfig, TransportConfig};
use wintermute_reach::digest::{PresenceTally, deliver_digest};

/// Build a fake-sendmail capture script.
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

/// AC8: With `enabled: false`, deliver_digest returns delivered=false.
#[test]
fn digest_ac8_disabled_yields_no_delivery() {
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
            enabled: false, // explicitly disabled
            send_hour: 20,
            contact_name: Some("Mom".to_string()),
        },
        ..Default::default()
    };

    let mut tally = PresenceTally::default();
    tally.record_summon(1_717_000_000);

    let result = deliver_digest(&cfg, &cfg.digest, &tally).expect("deliver_digest");

    assert!(
        !result.delivered,
        "disabled digest should not deliver: {result:?}"
    );

    // The capture file should NOT exist (no transport invocation occurred).
    assert!(
        !capture_path.exists(),
        "sendmail should not be invoked when digest is disabled"
    );
}

/// AC8: Default DigestConfig has enabled=false.
#[test]
fn digest_ac8_default_config_is_disabled() {
    let cfg = DigestConfig::default();
    assert!(
        !cfg.enabled,
        "DigestConfig default should have enabled=false"
    );
}

/// AC8: Config round-trips enabled=false through JSON.
#[test]
fn digest_ac8_disabled_config_roundtrips() {
    let cfg = DigestConfig {
        enabled: false,
        send_hour: 20,
        contact_name: None,
    };
    let json = serde_json::to_string(&cfg).expect("serialize");
    let back: DigestConfig = serde_json::from_str(&json).expect("deserialize");
    assert!(!back.enabled, "enabled=false should survive JSON round-trip");
}
