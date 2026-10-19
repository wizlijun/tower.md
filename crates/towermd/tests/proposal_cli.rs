//! Public CLI forwards only the source-bound ProposalInput and original UUID.
#[allow(dead_code)]
#[path = "support/finish_cli_fixture.rs"]
mod cli_fixture;
#[path = "support/cli_output.rs"]
mod cli_output;
#[allow(dead_code)]
#[path = "support/finish_fixture.rs"]
mod support;
use cli_fixture::Fixture;
use serde_json::{json, Value};
use std::{fs, process::Command};
use towermd_core::{
    catalog::Catalog,
    proposals::{Alternative, ProposalInput, ProposalSource},
    recovery_contract::recorded::request_event_eid,
    snapshot_service::read_draft,
};

fn shape() -> Value {
    json!({"source":{"kind":"branch","revision":"a".repeat(40),"base_revision":"b".repeat(40)},
        "reason":"明确变更原因","affected_intents":[],"affected_modules":[],"affected_interfaces":[],
        "affected_tasks":[],"affected_paths":[],"alternatives":[{"name":"保留","consequences":"保留现状"}],"recommendation":"采用明确变更"})
}
#[test]
fn propose_cli_rejects_invalid_input_and_authority_flags_before_creating_runtime() {
    let root = tempfile::tempdir().unwrap();
    let runtime = root.path().join("runtime");
    let path = root.path().join("proposal.json");
    let invoke = |extra: &[&str]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_towermd"));
        cmd.arg("propose")
            .arg(&path)
            .args(extra)
            .current_dir(root.path())
            .env("TOWERMD_RUNTIME_DIR", &runtime)
            .output()
            .unwrap()
    };
    let help = Command::new(env!("CARGO_BIN_EXE_towermd"))
        .args(["--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout).unwrap().contains("propose"));
    for fault in ["actor", "reason", "alternatives", "source"] {
        let mut input = shape();
        match fault {
            "actor" => input["actor"] = json!("human:injected"),
            "reason" => input["reason"] = json!(""),
            "alternatives" => input["alternatives"] = json!([]),
            _ => input["source"]["revision"] = json!("main"),
        }
        fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
        let output = invoke(&[]);
        assert!(!output.status.success(), "{fault}");
        assert!(output.stdout.is_empty());
        assert!(!runtime.exists(), "{fault} contacted the daemon");
    }
    fs::write(&path, serde_json::to_vec(&shape()).unwrap()).unwrap();
    for id in ["not-a-request", "0192f0aa-0000-4000-8000-000000000001"] {
        let output = invoke(&["--request-id", id]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!runtime.exists());
    }
    for args in [
        vec!["--actor", "human:owner"],
        vec!["--approve"],
        vec!["--skip"],
        vec!["--output-ref", "refs/heads/main"],
        vec!["--proof", "caller"],
    ] {
        assert!(!invoke(&args).status.success());
        assert!(!runtime.exists());
    }
}

#[test]
fn actual_propose_cli_preserves_original_receipt_private_source_and_old_run_history() {
    let f = Fixture::new(&["test -f alpha/a.py"]);
    f.snapshot_before_new_finish("T-finish");
    support::git(f.root.path(), &["rebase", "main"]);
    let catalog = Catalog::load(f.root.path()).unwrap();
    let run = f.json(&["task", "show"]);
    let module_path = catalog.object_paths["A-alpha"].clone();
    let before_bytes = catalog.read_file(&module_path).unwrap().unwrap();
    fs::write(
        f.root.path().join(&module_path),
        format!("{before_bytes}\nActual proposed module explanation.\n"),
    )
    .unwrap();
    support::git(f.root.path(), &["add", &module_path]);
    support::git(f.root.path(), &["commit", "-qm", "private proposal source"]);
    let input = ProposalInput {
        source: ProposalSource::Branch {
            revision: support::git(f.root.path(), &["rev-parse", "HEAD"]),
            base_revision: catalog.revision.clone(),
        },
        reason: "明确模块说明，保护层只作提案".into(),
        affected_intents: catalog
            .objects
            .iter()
            .filter(|(_, doc)| doc.doc_type() == Some("Intent"))
            .map(|(id, _)| id.clone())
            .collect(),
        affected_modules: vec!["A-alpha".into()],
        affected_interfaces: catalog
            .objects
            .iter()
            .filter(|(_, doc)| doc.doc_type() == Some("Interface"))
            .map(|(id, _)| id.clone())
            .collect(),
        affected_tasks: vec!["T-finish".into()],
        affected_paths: vec!["alpha/**".into(), "docs/tower/**".into()],
        alternatives: vec![Alternative {
            name: "保留".into(),
            consequences: "保留原模块说明".into(),
        }],
        recommendation: "采用明确说明".into(),
    };
    let path = f.runtime.path().join("proposal.json");
    fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
    let path = path.to_str().unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let main = support::git(f.root.path(), &["rev-parse", "main"]);
    let tag = support::git(f.root.path(), &["rev-parse", "refs/tags/towermd/B0"]);
    let before = f.outbox().confirmed().to_vec();
    let result = f.json(&["propose", path, "--request-id", &id]);
    assert_eq!(result["current_validity"]["state"], "current");
    assert_eq!(result["receipt"]["request_id"], id);
    let revision = result["receipt"]["result_ref"]["draft"]["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    let current = Catalog::load(f.root.path()).unwrap();
    let saved = read_draft(&current, &id).unwrap().unwrap();
    assert_eq!(saved.revision(), revision);
    assert_eq!(
        saved.frozen().content_hash,
        result["receipt"]["result_ref"]["draft"]["content_hash"]
    );
    let eid = request_event_eid(&id, "proposal.created").unwrap();
    let inputs = f.outbox();
    let actual = inputs
        .confirmed()
        .iter()
        .find(|event| event.eid == eid)
        .unwrap()
        .clone();
    assert!(!actual.stale);
    assert_eq!(actual.actor, "process:towermd-cli");
    assert_eq!(actual.host, "host");
    assert_eq!(serde_json::to_value(&actual.guards).unwrap(), run["leases"]);
    assert_eq!(
        actual.data["request_body"]["action"]["input"],
        serde_json::to_value(&input).unwrap()
    );
    assert_eq!(actual.data["result"]["binding"]["run"], run["id"]);
    assert!(inputs
        .confirmed()
        .iter()
        .filter(|event| !before.contains(event))
        .all(|event| event.kind == "proposal.created"));
    let again = f.json(&["propose", path, "--request-id", &id]);
    assert_eq!(again, result);
    assert_eq!(
        f.outbox()
            .confirmed()
            .iter()
            .filter(|event| event.eid == eid)
            .collect::<Vec<_>>(),
        vec![&actual]
    );
    assert_eq!(
        read_draft(&current, &id).unwrap().unwrap().frozen(),
        saved.frozen()
    );
    assert_eq!(support::git(f.root.path(), &["rev-parse", "main"]), main);
    assert_eq!(
        support::git(f.root.path(), &["rev-parse", "refs/tags/towermd/B0"]),
        tag
    );
    let mut changed = input.clone();
    changed.reason.push_str(" changed");
    fs::write(path, serde_json::to_vec(&changed).unwrap()).unwrap();
    let conflict = f
        .command(&["propose", path, "--request-id", &id])
        .output()
        .unwrap();
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains(&id));
    assert!(conflict.stdout.is_empty());
    assert_eq!(
        read_draft(&current, &id).unwrap().unwrap().revision(),
        revision
    );
    fs::write(path, serde_json::to_vec(&input).unwrap()).unwrap();
    f.json(&["task", "release"]);
    let before_retry = f.outbox().confirmed().to_vec();
    let historical = f.json(&["propose", path, "--request-id", &id]);
    assert_eq!(historical["receipt"], result["receipt"]);
    assert_eq!(historical["current_validity"]["state"], "historical_only");
    assert_eq!(f.outbox().confirmed(), before_retry);
    assert_eq!(
        read_draft(&current, &id).unwrap().unwrap().revision(),
        revision
    );
    assert_eq!(
        support::git(f.root.path(), &["rev-parse", "refs/tags/towermd/B0"]),
        tag
    );
}
