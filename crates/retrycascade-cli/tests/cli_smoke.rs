use std::fs;
use std::process::Command;

fn smoke_fixture() -> String {
    format!(
        "{}/../../tests/golden/resolved_smoke_config.json",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn resolved_smoke_config_produces_a_versioned_summary() {
    let output = Command::new(env!("CARGO_BIN_EXE_retrycascade-cli"))
        .arg(smoke_fixture())
        .output()
        .unwrap();

    assert!(output.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        summary["summary_schema_version"],
        "retrycascade-run-summary-v1.0.0"
    );
    assert_eq!(summary["lifecycle_ledger_closed"], true);
    assert_eq!(summary["summary_sha256"].as_str().unwrap().len(), 64);
}

#[test]
fn invalid_config_invariant_returns_nonzero() {
    let fixture = fs::read_to_string(smoke_fixture()).unwrap();
    let invalid = fixture.replace("\"keyed-chacha20-v2\"", "\"unsupported-rng\"");
    let path = std::env::temp_dir().join(format!(
        "retrycascade-invalid-config-{}.json",
        std::process::id()
    ));
    fs::write(&path, invalid).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_retrycascade-cli"))
        .arg(&path)
        .output()
        .unwrap();
    let _ = fs::remove_file(path);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported rng_version"));
}
