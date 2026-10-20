use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

fn towermd(runtime: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_towermd"))
        .args(args)
        .current_dir(runtime)
        .env("TOWERMD_RUNTIME_DIR", runtime)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn ensure_starts_one_loopback_daemon_and_stop_ends_it() {
    let runtime = tempfile::tempdir().unwrap();
    assert_eq!(
        towermd(runtime.path(), &["daemon", "status"])["running"],
        false
    );
    let first = towermd(runtime.path(), &["daemon", "ensure"]);
    assert_eq!(first["address"], "127.0.0.1");
    let second = towermd(runtime.path(), &["daemon", "ensure"]);
    assert_eq!(second["instance"], first["instance"]);
    assert_eq!(
        towermd(runtime.path(), &["daemon", "status"])["pid"],
        first["pid"]
    );
    assert_eq!(
        towermd(runtime.path(), &["daemon", "stop"])["running"],
        false
    );
    assert_eq!(
        towermd(runtime.path(), &["daemon", "status"])["running"],
        false
    );
}
