use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use towermd_core::main_writer::{
    self, WriteKind, WriteOutcome, WriteRequest,
};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Writer Test",
            "-c",
            "user.email=qa@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim_end().into()
}

fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-qb", "main"]);
    fs::write(temp.path().join("source.txt"), "original\n").unwrap();
    fs::create_dir(temp.path().join(".towermd")).unwrap();
    fs::write(temp.path().join(".towermd/config.yaml"),
        "project_id: writer-project\ndocs_dir: docs/tower\nhub: local\n").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "initial"]);
    temp
}

fn snapshot(root: &Path, epoch: u64, seq: u64) -> WriteRequest {
    use towermd_core::{hub::Hub, hub_protocol::{AcquireLease, HubProtocol, NewEvent}};
    let mut hub=Hub::open(&root.join(".git/writer-fixture-hub")).unwrap();
    let now=chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z").unwrap().timestamp_millis() as u64;
    if hub.contiguous_seq()==0 {
        let mut names=(1..epoch).map(|id|format!("module:fixture-{id}")).collect::<Vec<_>>();
        names.push("main".into());
        hub.lease_acquire(AcquireLease {host:"writer-test".into(), names,
            holder:"writer".into(),ttl_ms:900_000},now).unwrap();
    }
    while hub.contiguous_seq()<seq {
        let next=hub.contiguous_seq()+1;
        hub.append(NewEvent {eid:uuid::Uuid::now_v7().to_string(),host:"writer-test".into(),
            actor:"process:writer-test".into(),kind:"fixture.observed".into(),layer:"L5".into(),
            obj:format!("event-{next}"),data:serde_json::json!({}),guards:vec![]},now).unwrap();
    }
    let snapshot=towermd_core::snapshot::Snapshot::build("writer-project",None,&hub.events(0),seq,
        epoch,hub.epoch_high_water().max(epoch),|_|Ok(serde_json::Value::Null)).unwrap();
    WriteRequest {
        docs_dir: "docs/tower".into(),
        epoch,
        kind: WriteKind::Snapshot { seq, promote_from: None },
        changes: snapshot.files("docs/tower").unwrap(),
        message: "Tower.md snapshot".into(),
    }
}

fn written(outcome: WriteOutcome) -> (String, String) {
    match outcome {
        WriteOutcome::Written {
            commit, reference, ..
        } => (commit, reference),
        other => panic!("expected write: {other:?}"),
    }
}

fn slow(base: String) -> WriteRequest {
    WriteRequest {
        docs_dir: "docs/tower".into(),
        epoch: 3,
        kind: WriteKind::Slow {
            expected_base: base,
            tag: Some("refs/tags/towermd/B0".into()),
        },
        changes: BTreeMap::from([(
            "docs/tower/arch/A.md".into(),
            Some(b"---\ntype: Module\n---\n".to_vec()),
        )]),
        message: "Approve B0\n\nTowermd-Baseline: B0\nTowermd-Draft: d-test\n".into(),
    }
}

fn remote(root: &Path, directory: &Path) {
    git(directory, &["init", "--bare", "-q", "origin.git"]);
    let origin = directory.join("origin.git");
    git(root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(root, &["push", "-q", "origin", "main"]);
}

fn approved_publication(
    root: &Path,
) -> (
    towermd_devkit::baseline_publication::Publication,
    towermd_core::hub::Event,
) {
    use towermd_core::hub::{AppendReceipt, Hub};
    use towermd_core::hub_protocol::{HubProtocol, NewEvent};
    let mut publication = towermd_devkit::baseline_publication::Publication {
        removals: vec![],
        request_id: uuid::Uuid::now_v7().to_string(),
        draft_id: format!("d-{}", "a".repeat(64)),
        project_id: "qa".into(),
        source_revision: git(root, &["rev-parse", "HEAD"]),
        target: "B0".into(),
        prepared_at: "2026-10-21T00:00:00Z".into(),
        content_hash: String::new(),
        files: BTreeMap::from([(
            "docs/tower/arch/A.md".into(),
            b"---\ntype: Module\n---\n".to_vec(),
        )]),
    };
    publication.content_hash = publication.hash().unwrap();
    let mut hub = Hub::open(&root.join(".git/proof-hub")).unwrap();
    let receipt = hub
        .append(
            NewEvent {
                eid: publication.request_id.clone(),
                host: "qa".into(),
                actor: "human:local".into(),
                kind: "baseline.approved".into(),
                layer: "L1".into(),
                obj: "B0".into(),
                data: serde_json::to_value(&publication).unwrap(),
                guards: vec![],
            },
            1,
        )
        .unwrap();
    let AppendReceipt::Event(event) = receipt else {
        panic!()
    };
    (publication, *event)
}

#[test]
fn remote_publication_is_provable_immediately_without_a_local_baseline_tag() {
    let temp = fixture();
    let origin_temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    remote(root, origin_temp.path());
    let (publication, approval) = approved_publication(root);
    let (commit, _) = written(
        main_writer::write(root, &slow(publication.source_revision.clone()), || Ok(())).unwrap(),
    );
    assert_eq!(
        git(
            root,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/tags/towermd/B0"
            ]
        ),
        ""
    );
    publication.prove_commit(root, &commit, &approval).unwrap();
    let before = fs::read(root.join(".git/FETCH_HEAD")).unwrap();
    let readback = main_writer::read_publication(root, "refs/tags/towermd/B0")
        .unwrap()
        .unwrap();
    assert_eq!(readback.commit, commit);
    assert_eq!(readback.main_revision, commit);
    assert_eq!(fs::read(root.join(".git/FETCH_HEAD")).unwrap(), before);
    assert_eq!(
        git(root, &["rev-parse", "main"]),
        publication.source_revision
    );
    let (later, _) = written(main_writer::write(root, &snapshot(root, 3, 8), || Ok(())).unwrap());
    assert_eq!(
        main_writer::read_publication(root, "refs/tags/towermd/B0")
            .unwrap()
            .unwrap()
            .main_revision,
        later
    );
    publication.prove_commit(root, &commit, &approval).unwrap();
}

#[test]
fn discarded_push_receipt_is_recovered_from_origin_without_republishing() {
    let temp = fixture();
    let origin_temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    remote(root, origin_temp.path());
    let (publication, approval) = approved_publication(root);
    // The caller loses the response after the actual atomic push.
    main_writer::write(root, &slow(publication.source_revision.clone()), || Ok(())).unwrap();
    let remote_path = origin_temp.path().join("origin.git");
    let actual = git(&remote_path, &["rev-parse", "main"]);
    let count = git(&remote_path, &["rev-list", "--count", "main"]);
    fs::rename(&remote_path, origin_temp.path().join("offline.git")).unwrap();
    assert!(main_writer::read_publication(root, "refs/tags/towermd/B0").is_err());
    fs::rename(origin_temp.path().join("offline.git"), &remote_path).unwrap();
    let recovered = main_writer::read_publication(root, "refs/tags/towermd/B0")
        .unwrap()
        .unwrap();
    assert_eq!(recovered.commit, actual);
    publication
        .prove_commit(root, &recovered.commit, &approval)
        .unwrap();
    assert_eq!(git(&remote_path, &["rev-list", "--count", "main"]), count);
    assert_eq!(
        git(
            root,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/tags/towermd/B0"
            ]
        ),
        ""
    );
}
