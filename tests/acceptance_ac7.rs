//! AC7: ntfy and webhook backends compile behind Cargo features and are
//! excluded from the default build.
//!
//! This test verifies the feature-gating by inspecting #[cfg(feature)] guards
//! in the transport source. No live ntfy or webhook connections are made.

/// AC7: The transport source must contain feature guards for ntfy and webhook.
#[test]
fn acceptance_ac7_transport_source_has_feature_guards() {
    let transport_src = include_str!("../src/transport.rs");

    assert!(
        transport_src.contains("cfg(feature = \"ntfy\")"),
        "transport.rs must have #[cfg(feature = \"ntfy\")] guard"
    );
    assert!(
        transport_src.contains("cfg(feature = \"webhook\")"),
        "transport.rs must have #[cfg(feature = \"webhook\")] guard"
    );
}

/// AC7b: When built without features, the default transport is email only.
#[test]
fn acceptance_ac7b_default_build_is_email_only() {
    // Verify that ntfy and webhook are NOT compiled in by default.
    // We do this by checking that the `ntfy` and `webhook` feature flags are
    // not active in the current build (which uses the default feature set).
    assert!(
        !cfg!(feature = "ntfy"),
        "ntfy feature should not be active in default build"
    );
    assert!(
        !cfg!(feature = "webhook"),
        "webhook feature should not be active in default build"
    );
}

/// AC7c: The Cargo.toml declares ntfy and webhook as optional features.
#[test]
fn acceptance_ac7c_cargo_toml_has_optional_features() {
    let cargo_toml = include_str!("../Cargo.toml");
    assert!(
        cargo_toml.contains("ntfy"),
        "Cargo.toml must declare ntfy feature"
    );
    assert!(
        cargo_toml.contains("webhook"),
        "Cargo.toml must declare webhook feature"
    );
}
