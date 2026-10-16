use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use towermd_core::recovery_contract::{baseline::*, recorded::*, verification::*};

fn command() -> VerificationCommand {
    VerificationCommand {
        cwd: PathBuf::from("a\r\nb"),
        argv: vec!["/bin/sh".into(), "-c".into(), "echo x".into()],
    }
}
#[test]
fn commands_hash_uses_exact_canonical_structured_array() {
    let c = command();
    assert_eq!(
        commands_hash(std::slice::from_ref(&c)).unwrap(),
        "dd0cf40be7c84e85a07f9233c45c995dc618727f2700b130128d37d8a8d1dfea"
    );
    for changed in [
        VerificationCommand {
            cwd: "else".into(),
            ..c.clone()
        },
        VerificationCommand {
            argv: vec!["-c".into(), "/bin/sh".into(), "echo x".into()],
            ..c.clone()
        },
    ] {
        assert_ne!(
            commands_hash(std::slice::from_ref(&c)).unwrap(),
            commands_hash(&[changed]).unwrap()
        );
    }
    let c2 = VerificationCommand {
        cwd: ".".into(),
        argv: vec!["other".into()],
    };
    assert_ne!(
        commands_hash(&[c.clone(), c2.clone()]).unwrap(),
        commands_hash(&[c2, c]).unwrap()
    );
}
fn report() -> VerificationReport {
    VerificationReport {
        commit: "a".repeat(40),
        tree: "b".repeat(40),
        executions: vec![CommandExecution {
            command: command(),
            result: "passed".into(),
            exit_code: Some(0),
            timed_out: false,
            stdout_log: "stdout".into(),
            stderr_log: "stderr".into(),
            stdout_sha256: "c".repeat(64),
            stderr_sha256: "d".repeat(64),
            diagnostic: None,
        }],
    }
}
#[test]
fn report_hash_covers_every_field_and_execution_order() {
    let r = serde_json::to_value(report()).unwrap();
    let original = report_hash(&serde_json::from_value(r.clone()).unwrap()).unwrap();
    for key in ["commit", "tree"] {
        let mut changed = r.clone();
        changed[key] = json!("changed");
        assert_ne!(
            original,
            report_hash(&serde_json::from_value(changed).unwrap()).unwrap(),
            "{key}"
        );
    }
    for (key, value) in [
        ("command", json!({"cwd":"other","argv":[]})),
        ("result", json!("failed")),
        ("exit_code", json!(null)),
        ("timed_out", json!(true)),
        ("stdout_log", json!("other")),
        ("stderr_log", json!("other")),
        ("stdout_sha256", json!("e".repeat(64))),
        ("stderr_sha256", json!("f".repeat(64))),
        ("diagnostic", json!("why")),
    ] {
        let mut changed = r.clone();
        changed["executions"][0][key] = value;
        assert_ne!(
            original,
            report_hash(&serde_json::from_value(changed).unwrap()).unwrap(),
            "{key}"
        );
    }
    let mut two = report();
    two.executions.push(CommandExecution {
        stdout_log: "second".into(),
        ..two.executions[0].clone()
    });
    let before = report_hash(&two).unwrap();
    two.executions.reverse();
    assert_ne!(before, report_hash(&two).unwrap());
}
fn publication() -> (PublicationFreeze, BTreeMap<String, String>) {
    let texts = BTreeMap::from([("docs/tower/a.md".into(), "原文\r\n".into())]);
    (
        PublicationFreeze {
            project_id: "p".into(),
            draft_id: "d".into(),
            docs_dir: "docs/tower".into(),
            source_revision: "a".repeat(40),
            target_baseline: "towermd/B0".into(),
            base_directory: BTreeMap::from([(
                "docs/tower/a.md".into(),
                format!("100644 {}", "b".repeat(40)),
            )]),
            files: BTreeMap::from([(
                "docs/tower/a.md".into(),
                PublicationFile {
                    mode: "100644".into(),
                    sha256: "668ae9dfb44c250a10cdbf0f36f86a490c78a7ca9142698f75584b25e7797b52"
                        .into(),
                },
            )]),
            deletions: vec!["docs/tower/deleted.md".into()],
            questions: vec!["q2".into(), "q1".into()],
            risks: vec!["r".into()],
        },
        texts,
    )
}
#[test]
fn publication_hash_is_fixed_ten_length_prefixed_parts() {
    let (f, t) = publication();
    assert_eq!(
        publication_content_hash(&f, &t).unwrap(),
        "77f047601519e38377ab9cd8be48172807e4174bb7492321a81d780a35029a54"
    );
    let mut changed = f.clone();
    changed.questions.reverse();
    assert_ne!(
        publication_content_hash(&f, &t).unwrap(),
        publication_content_hash(&changed, &t).unwrap()
    );
}
#[test]
fn publication_rejects_unapproved_text_mode_path_or_deletions() {
    let (f, t) = publication();
    let mut bad = t.clone();
    bad.insert("extra".into(), "x".into());
    assert!(publication_content_hash(&f, &bad).is_err());
    let mut bad = t.clone();
    bad.values_mut().next().unwrap().push('\n');
    assert!(publication_content_hash(&f, &bad).is_err());
    for mode in ["120000", "160000", "0100644", "100644 "] {
        let mut bad = f.clone();
        bad.files.values_mut().next().unwrap().mode = mode.into();
        assert!(publication_content_hash(&bad, &t).is_err());
    }
    for path in ["../x", "/x", "a//b", "a/./b", "a/../b", "a\\b"] {
        let mut bad = f.clone();
        bad.deletions = vec![path.into()];
        assert!(publication_content_hash(&bad, &t).is_err());
    }
    for ds in [vec!["z", "a"], vec!["a", "a"]] {
        let mut bad = f.clone();
        bad.deletions = ds.into_iter().map(String::from).collect();
        assert!(publication_content_hash(&bad, &t).is_err());
    }
}
#[test]
fn publication_freeze_rechecks_original_content_hash() {
    let (f, t) = publication();
    let hash = publication_content_hash(&f, &t).unwrap();
    let draft = FrozenDraft {
        bundle: DraftBundle {
            id: f.draft_id.clone(),
            project_id: f.project_id.clone(),
            docs_dir: f.docs_dir.clone(),
            source_revision: f.source_revision.clone(),
            target_baseline: f.target_baseline.clone(),
            objects: BTreeMap::new(),
            canvas_text: String::new(),
            deletions: f.deletions.clone(),
            questions: f.questions.clone(),
            risks: f.risks.clone(),
        },
        files: f
            .files
            .iter()
            .map(|(p, v)| {
                (
                    p.clone(),
                    FrozenFile {
                        mode: v.mode.clone(),
                        sha256: v.sha256.clone(),
                        text: t[p].clone(),
                    },
                )
            })
            .collect(),
        base_directory: f.base_directory.clone(),
        content_hash: hash,
    };
    assert_eq!(publication_freeze(&draft).unwrap(), f);
    assert!(publication_freeze(&FrozenDraft {
        content_hash: "0".repeat(64),
        ..draft
    })
    .is_err());
}
#[test]
fn request_eid_requires_canonical_v7_and_keeps_kind_domain() {
    let u = "0192f0aa-0000-7000-8000-000000000001";
    assert_eq!(
        request_event_eid(u, "hold.reviewed").unwrap(),
        "git:b2516c4d713f8966970f2c28cbe700a9ecfd41cff5d3bf9ba1a278453e4deb33"
    );
    assert_ne!(
        request_event_eid(u, "hold.reviewed").unwrap(),
        request_event_eid(u, "baseline.approved").unwrap()
    );
    for bad in [
        "0192F0AA-0000-7000-8000-000000000001",
        "0192f0aa000070008000000000000001",
        "0192f0aa-0000-4000-8000-000000000001",
        "x",
    ] {
        assert!(request_event_eid(bad, "hold.reviewed").is_err());
    }
}

fn typed_recorded_values() -> Vec<serde_json::Value> {
    let tb = json!({"run":"R","task":"T","baseline":"towermd/B0","fences":[],"subject":{"kind":"plan","sha256":"a".repeat(64)}});
    let fact = json!({"binding":tb,"verdict":"approved","actor":"human:u","answer_eid":"event"});
    let bb = json!({"project_id":"p","object_id":"d","content_hash":"a".repeat(64),"target_baseline":"towermd/B0","draft_revision":"b".repeat(40)});
    let approval = json!({"approval_eid":"approved","binding":bb,"actor":"human:u","verdict":"approved","comment":"ok"});
    let prepared = json!({"prepared_eid":"prepared","publication_id":"approved","authority_revision":"b".repeat(40),"binding":bb,"content_hash":"a".repeat(64)});
    let result = json!({"commit":"c".repeat(40),"publication_id":"approved","approval_eid":"approved","content_hash":"a".repeat(64),"target":"towermd/B0","tag":"towermd/B0","tags":["towermd/B0"]});
    let body = |a| json!({"project_id":"p","actor":"human:u","host":"h","run":null,"action":a});
    let request = json!({"request_id":"u","body_hash":"a".repeat(64)});
    let receipt = |r| json!({"request_id":"u","body_hash":"a".repeat(64),"result_ref":r});
    vec![
        json!({"hold":{"type":"Hold","id":"H","body":{"kind":"review","binding":tb,"verdict":"approved","plan_content":"plan","source_commit":null,"source_path":null,"comment":"ok"},"opened_ms":1,"opened_seq":1,"reply":{"id":"reply","text":"ok","actor":"human:u","at_ms":2,"future_reply":42},"future_hold":42},"fact":fact,"comment":"ok","request_body":body(json!({"kind":"review","hold":"H","binding":tb,"verdict":"approved","comment":"ok"})),"p3_request":request,"p3_receipt":receipt(json!({"hold":"H","fact":fact,"verdict":"approved","comment":"ok"}))}),
        json!({"approval":approval,"request_body":body(json!({"kind":"review_baseline","binding":bb,"verdict":"approved","comment":"ok"})),"p3_request":request,"p3_receipt":receipt(approval)}),
        json!({"publication_id":"approved","approval_eid":"approved","binding":bb,"authority_revision":"b".repeat(40),"content_hash":"a".repeat(64),"request_body":body(json!({"kind":"prepare_publication","publication_id":"approved","approval_eid":"approved","binding":bb,"authority_revision":"b".repeat(40),"content_hash":"a".repeat(64)})),"p3_request":request,"p3_receipt":receipt(prepared)}),
        {
            let mut v = result.clone();
            v["request_body"] = body(json!({"kind":"record_baseline","result":result}));
            v["p3_request"] = request.clone();
            v["p3_receipt"] = receipt(result);
            v
        },
    ]
}
fn roundtrip(
    index: usize,
    value: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Error> {
    match index {
        0 => serde_json::from_value::<TaskReviewRecorded>(value).and_then(serde_json::to_value),
        1 => serde_json::from_value::<BaselineReviewRecorded>(value).and_then(serde_json::to_value),
        2 => serde_json::from_value::<PublicationPrepared>(value).and_then(serde_json::to_value),
        _ => serde_json::from_value::<BaselineCreated>(value).and_then(serde_json::to_value),
    }
}
#[test]
fn four_recorded_dtos_require_exact_envelopes_and_preserve_original_hold_extras() {
    for (index, value) in typed_recorded_values().into_iter().enumerate() {
        assert_eq!(roundtrip(index, value.clone()).unwrap(), value);
        for pointer in [
            "",
            "/request_body",
            "/request_body/action",
            "/p3_request",
            "/p3_receipt",
            "/p3_receipt/result_ref",
        ] {
            let mut bad = value.clone();
            bad.pointer_mut(pointer).unwrap()["future"] = json!(true);
            assert!(
                roundtrip(index, bad).is_err(),
                "index={index} pointer={pointer}"
            );
        }
        for key in value.as_object().unwrap().keys() {
            let mut bad = value.clone();
            bad.as_object_mut().unwrap().remove(key);
            assert!(
                roundtrip(index, bad).is_err(),
                "index={index} missing={key}"
            );
        }
        let mut bad = value.clone();
        bad["request_body"].as_object_mut().unwrap().remove("run");
        assert!(
            roundtrip(index, bad).is_err(),
            "nullable run must be explicit"
        );
    }
}
#[test]
fn verification_nullable_fields_and_machine_dtos_are_required_and_strict() {
    let value = serde_json::to_value(report()).unwrap();
    for key in ["exit_code", "diagnostic"] {
        let mut bad = value.clone();
        bad["executions"][0].as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<VerificationReport>(bad).is_err());
    }
    let mut bad = value.clone();
    bad["executions"][0]["future"] = json!(true);
    assert!(serde_json::from_value::<VerificationReport>(bad).is_err());
    let mut bad = serde_json::to_value(publication().0).unwrap();
    bad["future"] = json!(true);
    assert!(serde_json::from_value::<PublicationFreeze>(bad).is_err());
}

use towermd_core::{
    catalog::Catalog,
    config::Config,
    hub_protocol::NewEvent,
    init::{init, InitOptions},
    recovery_outbox::{RecoveryContext, RecoveryOutbox},
};
fn git(root: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=codex/gpt-6",
            "-c",
            "user.email=codex@towermd.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .into()
}
fn real_catalog() -> (tempfile::TempDir, Catalog) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q", "-b", "main"]);
    init(
        root.path(),
        &InitOptions {
            docs_dir: Some("docs/tower".into()),
        },
    )
    .unwrap();
    git(root.path(), &["add", "--all"]);
    git(root.path(), &["commit", "-qm", "contract fixture"]);
    let c = Catalog::load(root.path()).unwrap();
    (root, c)
}
fn event(host: &str) -> NewEvent {
    NewEvent {
        eid: "0192f0aa-0000-7000-8000-000000000001".into(),
        host: host.into(),
        actor: "process:test".into(),
        kind: "task.added".into(),
        layer: "L5".into(),
        obj: "T".into(),
        data: json!({"unknown":{"b":null,"a":[3,2,1]}}),
        guards: vec![],
    }
}
#[test]
fn pending_outbox_survives_real_file_reopen_and_keeps_full_data() {
    let (_root, c) = real_catalog();
    let storage = tempfile::tempdir().unwrap();
    let context = RecoveryContext::from_catalog(&c).unwrap();
    let mut outbox = RecoveryOutbox::open(storage.path(), context).unwrap();
    let e = event("host");
    outbox.append_pending(&e).unwrap();
    let first = outbox.read_inputs().unwrap();
    assert_eq!(first.pending(), std::slice::from_ref(&e));
    assert!(first.confirmed().is_empty());
    let digest = first.digest().to_owned();
    drop(outbox);
    let outbox =
        RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap()).unwrap();
    let inputs = outbox.read_inputs().unwrap();
    assert_eq!(inputs.pending(), &[e]);
    assert_eq!(inputs.digest(), digest);
    assert_eq!(inputs.high_water(), (0, 0));
}
#[test]
fn context_factory_rejects_self_reported_catalog_configuration() {
    let (_root, mut c) = real_catalog();
    c.config = Config::new("different", "docs/tower").unwrap();
    assert!(RecoveryContext::from_catalog(&c).is_err());
}

fn only_jsonl(directory: &std::path::Path) -> PathBuf {
    std::fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .unwrap()
}
#[test]
fn outbox_rejects_unknown_duplicate_fixed_fields_and_complete_corruption() {
    let (_root, c) = real_catalog();
    for change in [0, 1, 2, 3, 4] {
        let storage = tempfile::tempdir().unwrap();
        let mut o =
            RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap())
                .unwrap();
        o.append_pending(&event("h")).unwrap();
        let path = only_jsonl(storage.path());
        let original = std::fs::read_to_string(&path).unwrap();
        let changed = match change {
            0 => original.replacen("{", "{\"unknown\":true,", 1),
            1 => original.replacen("{", "{\"schema\":1,", 1),
            2 => original.replace("\"original\":{", "\"original\":{\"unknown\":true,"),
            3 => original.replace("\"docs_dir\":", "\"docs_dir\":\"other\",\"docs_dir\":"),
            _ => "not json\n".into(),
        };
        std::fs::write(path, changed).unwrap();
        assert!(o.read_inputs().is_err(), "case {change}");
    }
}
#[test]
fn outbox_pending_conflicts_rejected_before_writing_and_torn_tail_preserved() {
    let (_root, c) = real_catalog();
    let storage = tempfile::tempdir().unwrap();
    let mut o =
        RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap()).unwrap();
    let e = event("h");
    o.append_pending(&e).unwrap();
    let path = only_jsonl(storage.path());
    let before = std::fs::read(&path).unwrap();
    let mut changed = e.clone();
    changed.data = json!({"different":true});
    assert!(o.append_pending(&changed).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut bytes = before.clone();
    bytes.extend_from_slice(b"{\"state\":\"confirmed\"");
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(o.read_inputs().unwrap().pending(), std::slice::from_ref(&e));
    assert!(o
        .append_pending(&NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            ..e
        })
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
#[test]
fn outbox_never_queues_lease_system_events() {
    let (_root, c) = real_catalog();
    let storage = tempfile::tempdir().unwrap();
    let mut o =
        RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap()).unwrap();
    for kind in [
        "lease.renewed",
        "lease.granted",
        "lease.released",
        "lease.expired",
    ] {
        assert!(o
            .append_pending(&NewEvent {
                kind: kind.into(),
                ..event("h")
            })
            .is_err());
    }
    assert!(o.read_inputs().unwrap().pending().is_empty());
}
#[cfg(unix)]
#[test]
fn outbox_host_names_cannot_escape_or_follow_file_symlinks() {
    let (_root, c) = real_catalog();
    let storage = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let mut o =
        RecoveryOutbox::open(storage.path(), RecoveryContext::from_catalog(&c).unwrap()).unwrap();
    let e = event("../host");
    o.append_pending(&e).unwrap();
    let path = only_jsonl(storage.path());
    assert_eq!(path.parent().unwrap(), storage.path());
    std::fs::remove_file(&path).unwrap();
    let external = target.path().join("original");
    std::fs::write(&external, "untouched").unwrap();
    std::os::unix::fs::symlink(&external, &path).unwrap();
    assert!(o.append_pending(&e).is_err());
    assert_eq!(std::fs::read_to_string(external).unwrap(), "untouched");
    let link = target.path().join("directory-link");
    std::os::unix::fs::symlink(storage.path(), &link).unwrap();
    assert!(RecoveryOutbox::open(&link, RecoveryContext::from_catalog(&c).unwrap()).is_err());
}
#[test]
fn even_empty_input_digest_binds_the_actual_project_context() {
    let (_root, c) = real_catalog();
    let (_other, c2) = real_catalog();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let o = RecoveryOutbox::open(a.path(), RecoveryContext::from_catalog(&c).unwrap()).unwrap();
    let o2 = RecoveryOutbox::open(b.path(), RecoveryContext::from_catalog(&c2).unwrap()).unwrap();
    assert_ne!(
        o.read_inputs().unwrap().digest(),
        o2.read_inputs().unwrap().digest()
    );
}
