//! AC1: `wm-reach --help` lists `daemon`, `send`, `reply`, `test-transport`.

use std::process::Command;

/// AC1: help output lists all four required subcommand names.
#[test]
fn acceptance_ac1_help_lists_all_subcommands() {
    let output = Command::new(env!("CARGO_BIN_EXE_wm-reach"))
        .arg("--help")
        .output()
        .expect("failed to run wm-reach --help");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("daemon"),
        "help output missing 'daemon': {combined}"
    );
    assert!(
        combined.contains("send"),
        "help output missing 'send': {combined}"
    );
    assert!(
        combined.contains("reply"),
        "help output missing 'reply': {combined}"
    );
    assert!(
        combined.contains("test-transport"),
        "help output missing 'test-transport': {combined}"
    );
}
