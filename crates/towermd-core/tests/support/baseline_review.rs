#![allow(dead_code)]
use serde_json::json;
use std::{
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};
use towermd_core::{
    baseline_draft::{apply_edit, freeze, from_scan, DraftEdit, DraftObject, FrozenDraft},
    baseline_review::{review_view, ReviewAction},
    baseline_schema::BaselineSubmit,
    catalog::Catalog,
    coldstart::{capture_scan_input, ScanOptions},
    coldstart_evidence::{read_completed, PreparedColdstart},
    project_session::ProjectSession,
    recovery_contract::{baseline::Verdict, recorded::RequestBody},
    recovery_outbox::{RecoveryContext, RecoveryOutbox},
    request_ledger::{DraftSaveAction, PreparedDraftSaveRequest},
    snapshot_service::{read_draft, SnapshotWriter},
    verifier::{verify_sealed, VerificationRequest},
};
pub const NOW: u64 = 1_800_000_000_000;
pub struct Fixture {
    pub root: tempfile::TempDir,
    pub runtime: tempfile::TempDir,
    pub catalog: Arc<Catalog>,
    pub session: Arc<Mutex<ProjectSession>>,
    pub outbox: RecoveryOutbox,
    pub writer: Arc<SnapshotWriter>,
    pub frozen: FrozenDraft,
    pub revision: String,
}
pub fn git(root: &Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    let output = command
        .current_dir(root)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim_end().into()
}
pub fn fixture() -> Fixture {
    fixture_with_source_files(&[])
}
pub fn fixture_with_source_files(source_files: &[(&str, &str)]) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-qb", "main"]);
    towermd_core::init::init(
        root.path(),
        &towermd_core::init::InitOptions { docs_dir: None },
    )
    .unwrap();
    fs::create_dir_all(root.path().join("alpha")).unwrap();
    fs::write(root.path().join("alpha/a.py"), "# actual fixed code\n").unwrap();
    for (path, text) in source_files {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = towermd_core::okf::Document::parse(text).unwrap().to_text();
        fs::write(path, text).unwrap();
    }
    git(root.path(), &["add", "."]);
    git(root.path(), &["commit", "-qm", "actual initial source"]);
    git(root.path(), &["switch", "-qc", "review-fixture"]);
    let catalog = Arc::new(Catalog::load(root.path()).unwrap());
    let input = capture_scan_input(&catalog, &ScanOptions::default()).unwrap();
    let prepared = PreparedColdstart::prepare(input, "scan-review").unwrap();
    let input = prepared.input();
    let sealed = verify_sealed(&VerificationRequest {
        root: input.root.clone(),
        common_dir: input.common_dir.clone(),
        revision: input.revision.clone(),
        commands: prepared.commands().unwrap(),
        timeout: Duration::from_secs(5),
        log_root: input.common_dir.join("towermd/cache/verification"),
        cancelled: Arc::new(AtomicBool::new(false)),
    })
    .unwrap();
    let coldstart = prepared.freeze(&sealed, "host", NOW).unwrap();
    let mut outbox = RecoveryOutbox::open(
        &runtime.path().join("outbox"),
        RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let session = Arc::new(Mutex::new(
        ProjectSession::open(
            &catalog.common_dir.join("towermd"),
            &catalog,
            &outbox.read_inputs().unwrap(),
            NOW,
        )
        .unwrap(),
    ));
    ProjectSession::record_coldstart_shared(&session, &catalog, &mut outbox, &coldstart, NOW)
        .unwrap();
    let completed = read_completed(&catalog, &outbox.read_inputs().unwrap(), "scan-review")
        .unwrap()
        .unwrap();
    let modules=completed.scan()["modules"].as_array().unwrap().iter().map(|module|{
        let id=module["id"].as_str().unwrap();let fields=serde_yaml::to_string(&json!({"type":"Module","id":id,"title":id,"paths":module["paths"],"verify":[],"interfaces":[],"future":{"values":[2,1]}})).unwrap();DraftObject{id:id.into(),path:format!("arch/{id}.md"),text:format!("---\n{fields}---\nreal module body without final newline")}
    }).collect();
    let submit = BaselineSubmit {
        scan_id: completed.scan_id().into(),
        project_id: catalog.config.project_id().into(),
        source_revision: completed.input().revision.clone(),
        intents: vec![],
        modules,
        interfaces: vec![],
        acceptances: vec![],
        questions: vec!["question".into()],
        risks: vec![],
    };
    let bundle = from_scan(&catalog, &completed, submit.clone()).unwrap();
    let frozen = freeze(&catalog, &bundle).unwrap();
    let writer = Arc::new(SnapshotWriter::default());
    let request = PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: catalog.config.project_id().into(),
            actor: "process:towermd-cli".into(),
            host: "host".into(),
            run: None,
            action: DraftSaveAction::SubmitBaseline { submit },
        },
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let materialized = writer
        .save_draft(&session, &catalog, &frozen, &request, || Ok(NOW + 1))
        .unwrap();
    let revision = materialized.revision().to_owned();
    let saved = read_draft(&catalog, &bundle.id).unwrap().unwrap();
    assert_eq!(saved.revision(), revision);
    assert_eq!(saved.frozen(), &frozen);
    Fixture {
        root,
        runtime,
        catalog,
        session,
        outbox,
        writer,
        frozen,
        revision,
    }
}
pub fn action(f: &Fixture, verdict: Verdict) -> ReviewAction {
    let view = review_view(
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        &f.frozen.bundle.id,
    )
    .unwrap()
    .unwrap();
    assert!(
        view.publish_blockers.is_empty(),
        "{:?}",
        view.publish_blockers
    );
    ReviewAction {
        request_id: uuid::Uuid::now_v7().to_string(),
        binding: view.binding,
        verdict,
        comment: "human programmatic core review, not UI evidence".into(),
    }
}
pub fn next_save(f: &Fixture) -> (FrozenDraft, PreparedDraftSaveRequest) {
    let id = f.frozen.bundle.objects.keys().next().unwrap().clone();
    let edit = DraftEdit::Move {
        node: id,
        x: 512,
        y: 768,
    };
    let bundle = apply_edit(&f.frozen.bundle, &edit).unwrap();
    let frozen = freeze(&f.catalog, &bundle).unwrap();
    let request = PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: f.catalog.config.project_id().into(),
            actor: "human:owner".into(),
            host: "host".into(),
            run: None,
            action: DraftSaveAction::EditDraft {
                id: bundle.id.clone(),
                expected_revision: f.revision.clone(),
                edit,
            },
        },
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    (frozen, request)
}
