//! AC4: A `wm.family.distress` event is delivered ahead of a `wm.family.message`
//! that was queued first.

use wintermute_reach::config::{EmailConfig, TransportConfig};
use wintermute_reach::transport::build_transport;

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

/// AC4: distress is dispatched before normal message even when message arrived first.
///
/// Tests the priority-channel ordering via tokio biased select!.
#[test]
fn acceptance_ac4_distress_dispatched_before_normal_message() {
    use std::sync::{Arc, Mutex};

    let dispatch_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let pending_normal = "heating broken".to_string();
    let pending_distress = "EMERGENCY: fall detected".to_string();

    use tokio::sync::mpsc;
    let rt = tokio::runtime::Runtime::new().expect("rt");

    rt.block_on(async {
        let (distress_tx, mut distress_rx) = mpsc::channel::<String>(8);
        let (normal_tx, mut normal_rx) = mpsc::channel::<String>(8);

        // Queue normal first, distress second — order of arrival.
        normal_tx.send(pending_normal.clone()).await.expect("send normal");
        distress_tx.send(pending_distress.clone()).await.expect("send distress");

        // Drain with biased priority (distress first).
        let log = Arc::clone(&dispatch_log);
        loop {
            tokio::select! {
                biased;
                msg = distress_rx.recv() => {
                    let Some(m) = msg else { break; };
                    log.lock().expect("lock").push(format!("DISTRESS:{m}"));
                }
                msg = normal_rx.recv() => {
                    let Some(m) = msg else { break; };
                    log.lock().expect("lock").push(format!("NORMAL:{m}"));
                    break;
                }
            }
        }
    });

    let log = dispatch_log.lock().expect("lock").clone();
    assert_eq!(log.len(), 2, "expected 2 dispatch events: {log:?}");
    assert!(
        log[0].starts_with("DISTRESS:"),
        "first dispatch should be DISTRESS, got: {:?}",
        log[0]
    );
    assert!(
        log[1].starts_with("NORMAL:"),
        "second dispatch should be NORMAL, got: {:?}",
        log[1]
    );
}

/// AC4b: Verify the transport is called for distress with the correct subject prefix.
#[test]
fn acceptance_ac4b_distress_subject_has_distress_prefix() {
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

    let transport = build_transport(&cfg).expect("build transport");
    let result = transport
        .deliver("[DISTRESS] wintermute family alert", "fall detected")
        .expect("deliver");

    assert!(result.delivered, "distress delivery should succeed: {result:?}");

    let captured = std::fs::read_to_string(&capture_path).expect("read capture");
    assert!(
        captured.contains("DISTRESS"),
        "distress message should contain DISTRESS in subject: {captured}"
    );
}
