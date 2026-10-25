use super::*;
use std::{fs, path::PathBuf, process::Command, sync::atomic::AtomicBool};
use towermd_core::config::Config;
use towermd_core::hub::Fence;
use crate::task_model::{ModuleScope, PublishedBaseline, RunBinding, TaskDraft, TaskOrigin};

fn git_test(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(root)
        .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Finish Test", "-c", "user.email=finish@example.invalid",
            "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null"])
        .args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn write(root: &Path, path: &str, bytes: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, bytes).unwrap();
}
fn commit(root: &Path, message: &str) -> String {
    git_test(root, &["add", "."]);
    git_test(root, &["commit", "-qm", message]);
    git_test(root, &["rev-parse", "HEAD"])
}
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    baseline: VerifiedBaseline,
    projection: TaskProjection,
    leases: Vec<Lease>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_commands("  - directory: ''\n    command: 'printf tested; test -f src/app.txt'\n")
    }
    fn with_commands(commands: &str) -> Self {
        Self::build(commands, None)
    }
    fn with_gates(gates: &str) -> Self {
        Self::build("  - directory: ''\n    command: 'printf tested; test -f src/app.txt'\n", Some(gates))
    }
    fn build(commands: &str, gates: Option<&str>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        git_test(&root, &["init", "-qb", "main"]);
        write(&root, ".towermd/config.yaml", &Config::new("finish-project", "docs/tower").unwrap().to_yaml());
        if let Some(gates) = gates { write(&root, towermd_core::gates::GATES_PATH, gates); }
        for (id, path) in [("Z-main", "src/**"), ("A-touch", "lib/**")] {
            write(&root, &format!("docs/tower/arch/{id}.md"), &format!(
                "---\ntype: Module\nid: {id}\ntitle: {id}\npaths:\n  - '{path}'\nverification_commands:\n{commands}---\nApproved module.\n"
            ));
        }
        write(&root, "src/app.txt", "before\n");
        write(&root, "lib/helper.txt", "unchanged\n");
        write(&root, "README.md", "initial\n");
        let base = commit(&root, "approved fixture baseline");
        git_test(&root, &["tag", "towermd/B0", &base]);
        let baseline = VerifiedBaseline::from_published(PublishedBaseline {
            tag: "towermd/B0".into(), commit: base, approval_eid: "approval-event".into(), content_hash: "a".repeat(64),
            modules: BTreeMap::from([
                ("Z-main".into(), ModuleScope { paths: vec!["src/**".into()], high_risk: false }),
                ("A-touch".into(), ModuleScope { paths: vec!["lib/**".into()], high_risk: false }),
            ]), decisions: BTreeSet::new(),
        }).unwrap();
        let mut projection = TaskProjection::default();
        let added = task_model::prepare_add(&baseline, &projection, TaskDraft {
            id: "T-1".into(), title: "implement".into(), module: "Z-main".into(), touches: vec!["A-touch".into()],
            allowed_paths: vec!["src/**".into(), "lib/**".into()], depends_on: vec![],
            acceptance: vec!["test passes".into()], priority: 0, decisions: vec![], ..TaskDraft::default()
        }, TaskOrigin::Cli).unwrap();
        projection.apply(&added).unwrap();
        git_test(&root, &["checkout", "-qb", "t/T-1"]);
        let leases = vec![
            Lease { name: "module:A-touch".into(), holder: "R-1".into(), epoch: 5, expires_at: 5_000_000 },
            Lease { name: "module:Z-main".into(), holder: "R-1".into(), epoch: 11, expires_at: 5_000_000 },
        ];
        let claimed = task_model::prepare_claim(&baseline, &projection, "T-1", RunBinding {
            id: "R-1".into(), host: "test-host".into(), worktree: root.to_str().unwrap().into(),
            branch: "t/T-1".into(), self_reported: None,
        }, &leases, 1_000).unwrap();
        projection.apply(&claimed).unwrap();
        write(&root, "src/app.txt", "implementation\n");
        commit(&root, "implement task");
        Self { _temp: temp, root, baseline, projection, leases }
    }
    fn prepare(&self) -> Result<FrozenFinish, String> {
        prepare(&Catalog::load(&self.root)?, &self.baseline, &self.projection, "R-1", "E-1",
            &self.leases, 1_000, Limits::default())
    }
    fn finish(&self) -> Event {
        let input = self.prepare().unwrap();
        let report = crate::verifier::run(&verification_plan(&input, &self.root), &AtomicBool::new(false)).unwrap();
        let payload = complete(input, report, "finish-eid", &Catalog::load(&self.root).unwrap().common_dir).unwrap();
        assert!(payload.passed);
        envelope(1, FINISH_EVENT, "process:towermd-verifier", "finish-eid", &payload, &self.leases)
    }
    fn gate(&self, finish: &Event) -> Result<Event, String> {
        let gate = prepare_g5(&Catalog::load(&self.root)?, &self.baseline, &self.projection,
            finish, "g5-eid", &self.leases, 2_000)?;
        Ok(envelope(2, G5_EVENT, "process:towermd", "g5-eid", &gate, &self.leases))
    }
    fn amend_and_merge(&self, finish: &Event, gate: &Event) -> String {
        let trailer = trailers(&self.root, finish, gate).unwrap();
        git_test(&self.root, &["commit", "--amend", "-qm", &format!("implement task\n\n{trailer}")]);
        let tip = git_test(&self.root, &["rev-parse", "HEAD"]);
        git_test(&self.root, &["update-ref", "refs/heads/main", &tip]);
        tip
    }
}
fn envelope(seq: u64, kind: &str, actor: &str, eid: &str, value: &impl Serialize, leases: &[Lease]) -> Event {
    serde_json::from_value(serde_json::json!({
        "seq": seq, "eid": eid, "host": "test-host", "t": "2026-10-21T12:00:00Z",
        "actor": actor, "type": kind, "layer": "L4", "obj": "E-1",
        "data": serde_json::to_value(value).unwrap(),
        "epoch": leases.first().map(|lease| lease.epoch),
        "stale": false, "cost_usd": null,
        "guards": leases.iter().map(Lease::fence).collect::<Vec<_>>(),
    })).unwrap()
}
fn rewrite_finish(event: &mut Event, change: impl FnOnce(&mut FinishEvidence)) {
    let mut evidence: FinishEvidence = serde_json::from_value(event.data.clone()).unwrap();
    change(&mut evidence);
    event.data = serde_json::to_value(evidence).unwrap();
}

#[test]
fn real_verifier_amend_and_cold_clone_keep_original_tree_proof() {
    let fixture = Fixture::new();
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    let finish = fixture.finish();
    assert_eq!(index, fs::read(fixture.root.join(".git/index")).unwrap());
    let evidence: FinishEvidence = serde_json::from_value(finish.data.clone()).unwrap();
    assert_eq!(evidence.input.commands.len(), 1, "shared cwd/command runs once");
    assert_eq!(evidence.report.commands[0].argv.len(), 3);
    assert_eq!(evidence.logs[0].completion_eid, finish.eid);
    let gate = fixture.gate(&finish).unwrap();
    assert!(trailers(&fixture.root, &finish, &gate).unwrap().ends_with("Towermd-Epoch: 11\n"));
    let tip = fixture.amend_and_merge(&finish, &gate);
    assert_ne!(tip, evidence.input.revision);
    let proof = prove_merge(&fixture.root, &tip, &tip, &finish, &gate).unwrap();
    assert_eq!(proof.epoch, 11, "primary module, not alphabetically first touches");
    let destination = tempfile::tempdir().unwrap();
    let clone = destination.path().join("clone");
    git_test(destination.path(), &["clone", "-q", "--no-local", "--branch", "main",
        fixture.root.to_str().unwrap(), clone.to_str().unwrap()]);
    let old = Command::new("git").current_dir(&clone).args(["cat-file", "-e", &evidence.input.revision]).output().unwrap();
    assert!(!old.status.success(), "old pre-trailer commit must not be required by clone");
    assert!(!clone.join(".git/towermd/verify").exists());
    assert!(prove_merge(&clone, &tip, &tip, &finish, &gate).is_ok());
}

#[test]
fn dirty_worktree_and_uncommitted_untracked_file_are_rejected_without_mutation() {
    let fixture = Fixture::new();
    write(&fixture.root, "untracked.txt", "dirty");
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    assert!(fixture.prepare().unwrap_err().contains("未提交"));
    assert_eq!(index, fs::read(fixture.root.join(".git/index")).unwrap());
    assert_eq!(fs::read_to_string(fixture.root.join("untracked.txt")).unwrap(), "dirty");
}

#[test]
fn every_task_parent_edge_is_guarded_even_when_protected_edit_is_reverted() {
    let fixture = Fixture::new();
    write(&fixture.root, "docs/tower/tower.canvas", "{}");
    let forbidden = commit(&fixture.root, "forbidden checkpoint");
    git_test(&fixture.root, &["revert", "--no-edit", &forbidden]);
    assert!(fixture.prepare().unwrap_err().contains("G2"));
}

#[test]
fn a_sensitive_file_inside_the_allowed_paths_still_fails_g2() {
    let fixture = Fixture::new();
    write(&fixture.root, "src/deploy/.env", "TOKEN=x\n");
    let secret = commit(&fixture.root, "secret checkpoint");
    let error = fixture.prepare().unwrap_err();
    assert!(error.starts_with(G2_BLOCKED) && error.contains("src/deploy/.env") && error.contains("敏感文件"), "{error}");
    // Even removed again later, the edge that added it is guarded.
    git_test(&fixture.root, &["revert", "--no-edit", &secret]);
    assert!(fixture.prepare().unwrap_err().contains("敏感文件"));
}

#[test]
fn declared_behavior_tests_check_needs_a_test_with_the_change() {
    // Off by default: the plain fixture changes src/app.txt with no test.
    assert!(Fixture::new().prepare().is_ok());
    let fixture = Fixture::with_gates("behavior_tests:\n  code: [src/]\n  tests: [lib/tests/, \"*_test.txt\"]\n");
    let error = fixture.prepare().unwrap_err();
    assert!(error.starts_with(crate::behavior_tests::UNTESTED) && error.contains("src/app.txt"), "{error}");
    write(&fixture.root, "src/app_test.txt", "covers implementation\n");
    commit(&fixture.root, "add a test");
    assert!(fixture.prepare().is_ok());
    // A test added and then removed again does not count: the net change decides.
    let fixture = Fixture::with_gates("behavior_tests:\n  code: [src/]\n  tests: [lib/tests/]\n");
    write(&fixture.root, "lib/tests/app.txt", "test\n");
    commit(&fixture.root, "add a test");
    fs::remove_file(fixture.root.join("lib/tests/app.txt")).unwrap();
    commit(&fixture.root, "drop the test");
    assert!(fixture.prepare().unwrap_err().starts_with(crate::behavior_tests::UNTESTED));
    // An unreadable declaration fails closed.
    let fixture = Fixture::with_gates("behavior_tests: {code: [src/]}\n");
    assert!(fixture.prepare().unwrap_err().starts_with(crate::behavior_tests::UNTESTED));
}

#[test]
fn full_fences_and_approved_module_commands_are_required() {
    let mut fixture = Fixture::new();
    fixture.leases.pop();
    assert!(fixture.prepare().unwrap_err().contains("完整租约"));
    let fixture = Fixture::new();
    git_test(&fixture.root, &["checkout", "-q", "main"]);
    write(&fixture.root, "docs/tower/arch/Z-main.md",
        "---\ntype: Module\nid: Z-main\nverification_commands:\n  - directory: ''\n    command: 'true'\n---\nUnapproved.\n");
    commit(&fixture.root, "unapproved module command");
    git_test(&fixture.root, &["checkout", "-q", "t/T-1"]);
    git_test(&fixture.root, &["rebase", "main"]);
    assert!(fixture.prepare().unwrap_err().contains("已批准基线"));
}

#[test]
fn published_command_metadata_does_not_relax_execution_or_review_validation() {
    let published = "id: C-1\ndirectory: ''\ncommand: 'true'\nmodules: [Z-main]\n";
    assert!(serde_yaml::from_str::<ApprovedCommand>(published).is_ok());
    assert!(serde_yaml::from_str::<crate::review::ReviewedCommand>(published).is_err());
    for (commands, expected_error) in [
        ("  - directory: '../escape'\n    command: 'true'\n", "路径仅支持项目内字面文件"),
        ("  - directory: ''\n    command: '   '\n", "批准验证命令的 cwd 或命令无效"),
        ("  - directory: []\n    command: 'true'\n", "invalid type: sequence, expected a string"),
        ("  - directory: ''\n    command: []\n", "invalid type: sequence, expected a string"),
        ("  - command: 'true'\n", "missing field `directory`"),
        ("  - directory: ''\n", "missing field `command`"),
    ] {
        let fixture = Fixture::with_commands(commands);
        let error = fixture.prepare().unwrap_err();
        assert!(error.contains(expected_error), "{commands}: {error}");
    }
}

#[test]
fn report_arrays_cwd_and_original_completion_envelope_cannot_drift() {
    let fixture = Fixture::new();
    let finish = fixture.finish();
    let mut changed = finish.clone();
    rewrite_finish(&mut changed, |evidence| {
        evidence.report.commands[0].argv.push("silently omitted argument".into());
        evidence.report_sha256 = digest(&evidence.report).unwrap();
        evidence.logs = logs(&evidence.completion_eid, &evidence.report_sha256, &evidence.report);
    });
    assert!(fixture.gate(&changed).unwrap_err().contains("argv"));
    let mut changed = finish.clone();
    rewrite_finish(&mut changed, |evidence| {
        evidence.report.commands[0].directory = "src".into();
        evidence.report_sha256 = digest(&evidence.report).unwrap();
        evidence.logs = logs(&evidence.completion_eid, &evidence.report_sha256, &evidence.report);
    });
    assert!(fixture.gate(&changed).is_err());
    let mut changed = finish.clone();
    changed.eid = "different-completion".into();
    assert!(fixture.gate(&changed).is_err());
    let mut changed = finish.clone();
    changed.guards = vec![Fence { name: "module:Z-main".into(), holder: "R-1".into(), epoch: 11 }];
    assert!(fixture.gate(&changed).is_err());
    let mut changed = finish;
    rewrite_finish(&mut changed, |evidence| {
        evidence.input.manifest[0].oid = "0".repeat(40);
        evidence.input_sha256 = digest(&evidence.input).unwrap();
    });
    let error = fixture.gate(&changed).unwrap_err();
    assert!(error.contains("manifest"), "{error}");
}

#[test]
fn state_only_main_advance_requires_actual_rebase_and_new_g5_but_reuses_g3() {
    let fixture = Fixture::new();
    let finish = fixture.finish();
    let old_gate = fixture.gate(&finish).unwrap();
    git_test(&fixture.root, &["checkout", "-q", "main"]);
    write(&fixture.root, "docs/tower/state/state.json", "{\"seq\":1}\n");
    commit(&fixture.root, "snapshot");
    git_test(&fixture.root, &["checkout", "-q", "t/T-1"]);
    assert!(fixture.gate(&finish).is_err(), "ancestry, not only equal source code");
    git_test(&fixture.root, &["rebase", "main"]);
    let gate = fixture.gate(&finish).unwrap();
    let tip = fixture.amend_and_merge(&finish, &gate);
    assert!(prove_merge(&fixture.root, &tip, &tip, &finish, &gate).is_ok());
    assert!(prove_merge(&fixture.root, &tip, &tip, &finish, &old_gate).is_err());
}

#[test]
fn main_code_change_then_revert_still_requires_new_finish() {
    let fixture = Fixture::new();
    let finish = fixture.finish();
    git_test(&fixture.root, &["checkout", "-q", "main"]);
    write(&fixture.root, "README.md", "code/config/other main change\n");
    let changed = commit(&fixture.root, "main advance");
    git_test(&fixture.root, &["revert", "--no-edit", &changed]);
    git_test(&fixture.root, &["checkout", "-q", "t/T-1"]);
    git_test(&fixture.root, &["rebase", "main"]);
    assert!(fixture.gate(&finish).unwrap_err().contains("必须重新 finish"));
}

#[test]
fn squash_before_new_g5_reuses_tree_evidence_without_retesting() {
    let fixture = Fixture::new();
    write(&fixture.root, "lib/helper.txt", "second checkpoint\n");
    commit(&fixture.root, "checkpoint");
    let finish = fixture.finish();
    let old_gate = fixture.gate(&finish).unwrap();
    git_test(&fixture.root, &["reset", "--soft", "main"]);
    git_test(&fixture.root, &["commit", "-qm", "squashed task"]);
    let gate = fixture.gate(&finish).unwrap();
    let tip = fixture.amend_and_merge(&finish, &gate);
    assert!(prove_merge(&fixture.root, &tip, &tip, &finish, &gate).is_ok());
    assert!(prove_merge(&fixture.root, &tip, &tip, &finish, &old_gate).is_err());
}

#[test]
fn side_task_tip_in_illegal_merge_is_not_an_admission() {
    let fixture = Fixture::new();
    let finish = fixture.finish();
    let gate = fixture.gate(&finish).unwrap();
    let trailer = trailers(&fixture.root, &finish, &gate).unwrap();
    git_test(&fixture.root, &["commit", "--amend", "-qm", &format!("task\n\n{trailer}")]);
    let side = git_test(&fixture.root, &["rev-parse", "HEAD"]);
    git_test(&fixture.root, &["checkout", "-q", "main"]);
    write(&fixture.root, "README.md", "parallel main change\n");
    commit(&fixture.root, "parallel");
    git_test(&fixture.root, &["merge", "--no-ff", "-m", "illegal merge", "t/T-1"]);
    let main = git_test(&fixture.root, &["rev-parse", "HEAD"]);
    assert!(prove_merge(&fixture.root, &main, &side, &finish, &gate).unwrap_err().contains("first-parent"));
    assert!(prove_merge(&fixture.root, &main, &main, &finish, &gate).is_err());
}

#[test]
fn actual_main_admission_preserves_collaborator_trailers_outside_tower_proof() {
    let fixture = Fixture::new();
    let finish = fixture.finish();
    let gate = fixture.gate(&finish).unwrap();
    let trailer = trailers(&fixture.root, &finish, &gate).unwrap();
    let collaborators = "Co-Authored-By: First Contributor <first@example.invalid>\nCo-Authored-By: Second Contributor <second@example.invalid>\nSigned-off-by: Reviewer <reviewer@example.invalid>\n";
    for block in [format!("{trailer}{collaborators}"), format!("{collaborators}{trailer}")] {
        git_test(&fixture.root, &["commit", "--amend", "-qm", &format!("implement task\n\n{block}")]);
        let tip = git_test(&fixture.root, &["rev-parse", "HEAD"]);
        git_test(&fixture.root, &["update-ref", "refs/heads/main", &tip]);
        let original = git_test(&fixture.root, &["show", "-s", "--format=%B", &tip]);
        let proof = prove_merge(&fixture.root, &tip, &tip, &finish, &gate).unwrap();
        assert_eq!(proof.admission_commit, tip);
        assert_eq!(original, git_test(&fixture.root, &["show", "-s", "--format=%B", &tip]));
        assert!(original.contains(collaborators.trim_end()));
    }
}

#[test]
fn missing_or_duplicate_final_trailers_and_expired_execution_are_rejected() {
    let fixture = Fixture::new();
    let finish = fixture.finish();
    let gate = fixture.gate(&finish).unwrap();
    let trailer = trailers(&fixture.root, &finish, &gate).unwrap();
    for malformed in [
        trailer.replace("Towermd-Epoch: 11\n", ""),
        format!("{trailer}Towermd-Epoch: 11\n"),
        trailer.replace("Towermd-Epoch: 11\n", "Towermd-Epoch: 12\n"),
        format!("{trailer}Towermd-Unrecognized: value\n"),
        format!("{trailer}TOWERMD-Unrecognized: value\n"),
        format!("{trailer}\nCo-Authored-By: Later Block <later@example.invalid>\n"),
        format!("{trailer}not a trailer\n"),
        format!("{trailer}Bad Key: value\n"),
        format!("{trailer}: value\n"),
        format!("{trailer}Co-Authored-By: \n"),
    ] {
        git_test(&fixture.root, &["commit", "--amend", "-qm", &format!("task\n\n{malformed}")]);
        let tip = git_test(&fixture.root, &["rev-parse", "HEAD"]);
        git_test(&fixture.root, &["update-ref", "refs/heads/main", &tip]);
        assert!(prove_merge(&fixture.root, &tip, &tip, &finish, &gate).is_err());
    }
    let catalog = Catalog::load(&fixture.root).unwrap();
    assert!(prepare_g5(&catalog, &fixture.baseline, &fixture.projection, &finish,
        "late-g5", &fixture.leases, 1_000 + VERIFYING_TTL_MS).unwrap_err().contains("期限"));
}

#[test]
fn changed_local_log_prevents_producing_completion_evidence() {
    let fixture = Fixture::new();
    let input = fixture.prepare().unwrap();
    let report = crate::verifier::run(&verification_plan(&input, &fixture.root), &AtomicBool::new(false)).unwrap();
    let common = Catalog::load(&fixture.root).unwrap().common_dir;
    fs::write(common.join(report.commands[0].log.as_ref().unwrap()), "different log bytes").unwrap();
    assert!(complete(input, report, "finish-eid", &common).unwrap_err().contains("摘要"));
}

#[test]
fn failed_verification_is_preserved_but_cannot_emit_trailers_or_merge() {
    let fixture = Fixture::new();
    // The approved command requires this file. Deleting it is within task scope
    // and passes G2, but the real shared verifier must fail G3.
    fs::remove_file(fixture.root.join("src/app.txt")).unwrap();
    commit(&fixture.root, "remove required file");
    let input = fixture.prepare().unwrap();
    let report = crate::verifier::run(&verification_plan(&input, &fixture.root), &AtomicBool::new(false)).unwrap();
    let payload = complete(input, report, "finish-eid", &Catalog::load(&fixture.root).unwrap().common_dir).unwrap();
    assert!(!payload.passed);
    assert_eq!(payload.report.commands[0].status, VerificationStatus::Failed);
    let finish = envelope(1, FINISH_EVENT, "process:towermd-verifier", "finish-eid", &payload, &fixture.leases);
    assert!(fixture.gate(&finish).unwrap_err().contains("G3"));
}

#[test]
fn published_scan_commands_survive_unchanged_review_and_real_task_finish() {
    use crate::{baseline, baseline_publication, cold_start, review, review_verification};
    use towermd_core::main_writer;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    git_test(&root, &["init", "-qb", "main"]);
    write(&root, ".towermd/config.yaml", &Config::new("finish-project", "docs/tower").unwrap().to_yaml());
    write(&root, "src/app.rs", "fn before() {}\n");
    write(&root, "Makefile", "test:\n\t@test -f src/app.rs\n");
    commit(&root, "source before cold start");
    let catalog = Catalog::load(&root).unwrap();
    let mut scan = cold_start::scan(&catalog, "scan-published-finish").unwrap();
    let report = crate::verifier::run(&VerificationPlan {
        root: root.clone(), id: scan.id.clone(), revision: scan.revision.clone(), tree: scan.tree.clone(),
        commands: scan.commands.clone(), modules: scan.modules.clone(), limits: Limits::default(),
    }, &AtomicBool::new(false)).unwrap();
    cold_start::apply_verification(&mut scan, report).unwrap();
    let base = baseline::synthesize(&scan, baseline::template(&scan).unwrap(), "test-harness/1",
        "2026-10-21T00:00:00Z", &format!("d-{}", "b".repeat(64))).unwrap();
    let actions = scan.modules.iter().map(|module| review::ReviewAction::Commands {
        module_id: module.id.clone(), commands: scan.commands.iter()
            .filter(|command| module.command_ids.contains(&command.id))
            .map(|command| review::ReviewedCommand {
                directory: command.directory.clone(), command: command.command.clone(),
            }).collect(),
    }).collect::<Vec<_>>();
    let mut draft = review::replay(&base, &actions, "human:local", &format!("d-{}", "c".repeat(64)),
        "2026-10-21T00:01:00Z", &BTreeMap::new()).unwrap();
    let plan = review_verification::plan(&draft, &root, "review-published-finish", Limits::default()).unwrap();
    let report = crate::verifier::run(&plan, &AtomicBool::new(false)).unwrap();
    let verification = review_verification::ReviewVerification {
        parent_id: draft.id.clone(), input_sha256: review_verification::input_sha256(&draft, &plan.limits).unwrap(),
        run_id: plan.id, limits: plan.limits, report,
    };
    review_verification::apply(&mut draft, verification).unwrap();
    let publication = baseline_publication::prepare(&draft, &catalog.common_dir,
        &uuid::Uuid::now_v7().to_string(), "B0", "2026-10-21T00:02:00Z").unwrap();
    let mut approval = envelope(1, "baseline.approved", "human:local", &publication.request_id, &publication, &[]);
    approval.layer = "L1".into();
    approval.obj = "B0".into();
    git_test(&root, &["checkout", "-qb", "parking"]);
    let written = main_writer::write(&root, &main_writer::WriteRequest {
        docs_dir: "docs/tower".into(), epoch: 1,
        kind: main_writer::WriteKind::Slow {
            expected_base: publication.source_revision.clone(), tag: Some("refs/tags/towermd/B0".into()),
        },
        changes: publication.files.iter().map(|(path, bytes)| (path.clone(), Some(bytes.clone()))).collect(),
        message: "publish approved baseline fixture".into(),
    }, || Ok(())).unwrap();
    let main_writer::WriteOutcome::Written { commit: published, .. } = written else { panic!("publication superseded") };
    let baseline = baseline_publication::verified_baseline(&root, &published, &approval).unwrap();
    git_test(&root, &["reset", "--hard", "main"]);
    let module = scan.modules.iter().find(|module| module.files.contains(&"src/app.rs".into())).unwrap();
    let object = publication.files.values().filter_map(|bytes| std::str::from_utf8(bytes).ok())
        .filter_map(|text| Document::parse(text).ok())
        .find(|document| document.get_str("id") == Some(module.id.as_str())).unwrap();
    let commands: Vec<ScanCommand> = serde_yaml::from_value(object.get("verification_commands").unwrap().clone()).unwrap();
    assert!(!commands.is_empty(), "real publication must retain scan command metadata");
    assert!(!commands[0].id.is_empty());
    assert!(commands[0].modules.contains(&module.id));
    let mut projection = TaskProjection::default();
    let added = task_model::prepare_add(&baseline, &projection, TaskDraft {
        id: "T-1".into(), title: "implement".into(), module: module.id.clone(), touches: vec![],
        allowed_paths: vec!["src/app.rs".into()], depends_on: vec![], acceptance: vec!["test passes".into()],
        priority: 0, decisions: vec![], ..TaskDraft::default()
    }, TaskOrigin::Cli).unwrap();
    projection.apply(&added).unwrap();
    git_test(&root, &["checkout", "-qb", "t/T-1"]);
    let leases = vec![Lease { name: format!("module:{}", module.id), holder: "R-1".into(), epoch: 2, expires_at: 5_000_000 }];
    let claimed = task_model::prepare_claim(&baseline, &projection, "T-1", RunBinding {
        id: "R-1".into(), host: "test-host".into(), worktree: root.to_str().unwrap().into(),
        branch: "t/T-1".into(), self_reported: None,
    }, &leases, 1_000).unwrap();
    projection.apply(&claimed).unwrap();
    write(&root, "src/app.rs", "fn implemented() {}\n");
    commit(&root, "implement task");
    let fixture = Fixture { _temp: temp, root, baseline, projection, leases };
    let finish = fixture.finish();
    let gate = fixture.gate(&finish).unwrap();
    let tip = fixture.amend_and_merge(&finish, &gate);
    assert!(prove_merge(&fixture.root, &tip, &tip, &finish, &gate).is_ok());
}
