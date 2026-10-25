//! WP-X: raw output goes to an Arca dataset when one is set up, otherwise
//! it stays local; a missing or failing `arca` never fails the archive.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use towermd_devkit::artifacts::{archive, unsynced, Store};
use towermd_devkit::runner;

/// A stand-in `arca` that logs its arguments and answers like the plumbing.
fn mock_arca(dir: &Path, fail_add: bool) -> PathBuf {
    let log = dir.join("arca.log");
    let script = format!("#!/bin/sh\necho \"$@\" >> '{}'\ncase \"$1\" in\n  --version) echo 'arca 0.3.0' ;;\n  add) {} ;;\n  resolve) echo '{{\"path\":\"'\"$2\"'\",\"hash\":\"b3-abc\"}}' ;;\n  doctor) echo '{{\"ok\":true,\"unsynced\":4}}' ;;\n  *) exit 2 ;;\nesac\n",
        log.display(), if fail_add { "echo 'no space' >&2; exit 1" } else { "true" });
    let path = dir.join("arca");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn raw(common: &Path) {
    let dir = runner::run_dir(common, "R-1").join("1");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(runner::RAW), "{\"type\":\"result\"}\n").unwrap();
    fs::write(dir.join(runner::STDERR), "").unwrap();
}

#[test]
fn without_arca_raw_output_stays_local_and_says_so() {
    let temp = tempfile::tempdir().unwrap();
    let (root, common) = (temp.path().join("repo"), temp.path().join("repo/.git"));
    raw(&common);
    let record = archive(&root, "docs/tower", &common, None, "R-1", "T-1", 1);
    assert_eq!(record.store, Store::Local);
    assert_eq!(record.files.len(), 2);
    assert_eq!(record.files[0].path, "towermd/runs/R-1/1/output.jsonl");
    assert_eq!(record.files[0].sha256.len(), 64);
    assert!(record.note.unwrap().contains("没有安装 Arca"));
}

#[test]
fn arca_is_used_only_for_a_dataset_a_person_set_up_and_failures_fall_back() {
    let temp = tempfile::tempdir().unwrap();
    let (root, common) = (temp.path().join("repo"), temp.path().join("repo/.git"));
    raw(&common);
    let arca = mock_arca(temp.path(), false);
    let record = archive(&root, "docs/tower", &common, Some(&arca), "R-1", "T-1", 1);
    assert_eq!(record.store, Store::Local, "Tower.md never sets up a dataset");
    assert!(record.note.unwrap().contains("arca setup"));
    assert!(!root.join("docs/tower/artifacts/R-1").exists());

    fs::create_dir_all(root.join("docs/tower/artifacts/.arca")).unwrap();
    let record = archive(&root, "docs/tower", &common, Some(&arca), "R-1", "T-1", 1);
    assert_eq!(record.store, Store::Arca);
    assert_eq!(record.files[0].path, "docs/tower/artifacts/R-1/1/output.jsonl");
    assert_eq!(record.files[0].arca.as_deref(), Some("b3-abc"));
    assert_eq!(fs::read(root.join(&record.files[0].path)).unwrap(), b"{\"type\":\"result\"}\n");
    let log = fs::read_to_string(temp.path().join("arca.log")).unwrap();
    assert!(log.contains("add docs/tower/artifacts/R-1/1/output.jsonl"), "{log}");
    assert!(!log.contains("push") && !log.contains("gc"), "{log}");
    assert_eq!(unsynced(&arca, &root), Some(4));

    fs::create_dir_all(temp.path().join("broken")).unwrap();
    let broken = mock_arca(&temp.path().join("broken"), true);
    let record = archive(&root, "docs/tower", &common, Some(&broken), "R-1", "T-1", 1);
    assert_eq!(record.store, Store::Local, "a failing arca falls back to local");
    assert_eq!(record.files[0].path, "towermd/runs/R-1/1/output.jsonl");
    assert!(record.note.unwrap().contains("no space"));
}
