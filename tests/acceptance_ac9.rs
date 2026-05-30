//! AC9: systemd unit `wm-reach.service` ExecStart matches the install bin path.

use std::path::Path;

/// AC9: The service unit file exists and ExecStart points at the right binary name.
#[test]
fn acceptance_ac9_service_unit_execstart_matches_install_path() {
    let unit_path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/dist/wm-reach.service"));
    assert!(
        unit_path.exists(),
        "dist/wm-reach.service must exist; path: {}",
        unit_path.display()
    );

    let content = std::fs::read_to_string(unit_path).expect("read service unit");

    // ExecStart must reference wm-reach (the binary name, not the crate name).
    assert!(
        content.contains("wm-reach"),
        "ExecStart must reference 'wm-reach': {content}"
    );

    // Specifically ExecStart must not point at ~/.cargo/bin (cargo-vs-local drift).
    assert!(
        !content.contains(".cargo/bin"),
        "ExecStart must not point at .cargo/bin (use .local/bin): {content}"
    );

    // The expected install path is /home/jsy/.local/bin/wm-reach or ~/.local/bin/wm-reach.
    assert!(
        content.contains(".local/bin/wm-reach"),
        "ExecStart must reference .local/bin/wm-reach: {content}"
    );
}
