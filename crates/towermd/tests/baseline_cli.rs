use std::process::Command;

#[test]
fn cli_submission_validates_the_shared_schema_before_contacting_a_daemon() {
    let root = tempfile::tempdir().unwrap();
    let runtime = root.path().join("runtime");
    let input = root.path().join("input.json");
    std::fs::write(&input, r#"{"scan_id":"scan","actor":"human:injected"}"#).unwrap();
    let bin = env!("CARGO_BIN_EXE_towermd");
    let help = Command::new(bin)
        .args(["baseline", "--help"])
        .output()
        .unwrap();
    assert!(String::from_utf8(help.stdout).unwrap().contains("submit"));
    let output = Command::new(bin)
        .args(["baseline", "submit"])
        .arg(&input)
        .current_dir(root.path())
        .env("TOWERMD_RUNTIME_DIR", &runtime)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("$.project_id"));
    assert!(!runtime.exists());
    assert!(!Command::new(bin)
        .args(["baseline", "submit"])
        .arg(&input)
        .args(["--actor", "human:owner"])
        .output()
        .unwrap()
        .status
        .success());
}

#[test]
fn baseline_schema_is_the_shared_offline_schema_without_human_commands() {
    let root = tempfile::tempdir().unwrap();
    let runtime = root.path().join("runtime");
    let bin = env!("CARGO_BIN_EXE_towermd");
    let output = Command::new(bin)
        .args(["baseline", "schema"])
        .current_dir(root.path())
        .env("TOWERMD_RUNTIME_DIR", &runtime)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::from_str::<serde_json::Value>(towermd_core::baseline_schema::schema()).unwrap()
    );
    assert!(!runtime.exists());
    for args in [
        vec!["baseline", "approve", "D"],
        vec!["baseline", "reject", "D"],
        vec!["baseline", "verify"],
        vec!["baseline", "schema", "--actor", "human:owner"],
    ] {
        assert!(!Command::new(bin)
            .args(args)
            .current_dir(root.path())
            .env("TOWERMD_RUNTIME_DIR", &runtime)
            .output()
            .unwrap()
            .status
            .success());
    }
    assert!(!runtime.exists());
}
