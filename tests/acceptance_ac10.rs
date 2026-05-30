//! AC10: No secret (SMTP password, ntfy token) is logged; config reads from files,
//! never hard-coded.
//!
//! This is a grep-clean test against the src/ tree.

use std::path::Path;

/// AC10: Source files must not contain credential patterns.
#[test]
fn acceptance_ac10_source_has_no_hardcoded_credentials() {
    let forbidden_patterns = [
        // Common credential key names that would indicate hardcoding.
        "smtp_password",
        "smtppassword",
        "SMTP_PASSWORD",
        "ntfy_token",
        "NTFY_TOKEN",
        "api_key",
        "API_KEY",
        "Bearer ",
        "password = \"",
        "token = \"",
        "secret = \"",
    ];

    let src_dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
    let mut violations: Vec<String> = vec![];

    scan_dir(src_dir, &forbidden_patterns, &mut violations);

    assert!(
        violations.is_empty(),
        "Hardcoded credential pattern found:\n{}",
        violations.join("\n")
    );
}

fn scan_dir(dir: &Path, patterns: &[&str], violations: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, patterns, violations);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            for &pat in patterns {
                if content.contains(pat) {
                    violations.push(format!("{}: contains '{pat}'", path.display()));
                }
            }
        }
    }
}

/// AC10b: Config type does not derive Debug in a way that would expose secrets.
///
/// The EmailConfig struct holds `to` / `from` / `sendmail` — none are secrets
/// in the threat model. SMTP password would be in smtp_host (optional) or a
/// future credential field. The test documents this.
#[test]
fn acceptance_ac10b_config_struct_has_no_secret_fields() {
    // EmailConfig fields: to, from, sendmail, smtp_host.
    // None of these are secret credentials; smtp_host is a host name.
    // This test documents the invariant: if a secret field is ever added,
    // it must use a wrapper type that redacts its Debug output.
    let cfg_src = include_str!("../src/config.rs");
    assert!(
        !cfg_src.contains("smtp_password"),
        "smtp_password must not appear as a struct field in config.rs"
    );
    assert!(
        !cfg_src.contains("ntfy_token"),
        "ntfy_token must not appear as a struct field in config.rs"
    );
}
