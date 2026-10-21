use std::{fs, path::Path, process::Command};
use serde_json::json;
use towermd_core::{config::Config, events, hub::Hub, hub_protocol::*, main_resolution::{self, Operation, Preview}, main_writer::{self, WriteOutcome}};

fn git(root: &Path, args: &[&str]) -> String {
    let result = Command::new("git").current_dir(root)
        .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Resolution Test", "-c", "user.email=qa@localhost", "-c", "commit.gpgsign=false"])
        .args(args).output().unwrap();
    assert!(result.status.success(), "{args:?}: {}", String::from_utf8_lossy(&result.stderr));
    String::from_utf8(result.stdout).unwrap().trim_end().into()
}
fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path); fs::create_dir_all(file.parent().unwrap()).unwrap(); fs::write(file, text).unwrap();
}
fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "."]); git(root, &["commit", "-qm", message]); git(root, &["rev-parse", "HEAD"])
}
fn fixture() -> (tempfile::TempDir, String, String, Hub, Fence) {
    let temp = tempfile::tempdir().unwrap(); let root = temp.path();
    git(root, &["init", "-qb", "main"]);
    write(root, ".towermd/config.yaml", &Config::new("resolution-project", "docs/tower").unwrap().to_yaml());
    write(root, "docs/tower/arch/A.md", "---\ntype: Module\nid: A\n---\nOriginal\n");
    write(root, "source.txt", "original\n");
    let anchor = commit(root, "enable project with imported context");
    write(root, "docs/tower/arch/A.md", "---\ntype: Module\nid: A\n---\nEdited manually\n");
    let target = commit(root, "manual slow change");
    let mut hub = Hub::open(&root.join(".git/test-hub")).unwrap();
    let fence = hub.lease_acquire(AcquireLease { host: "test".into(), names: vec!["main".into()],
        holder: "daemon".into(), ttl_ms: 900_000 }, 1000).unwrap().leases[0].fence();
    (temp, anchor, target, hub, fence)
}
fn preview(root: &Path, main: &str, target: &str, operation: Operation) -> Preview {
    main_resolution::prepare(root, main, "resolution-project", "docs/tower", target, operation,
        &uuid::Uuid::now_v7().to_string()).unwrap()
}
fn append(hub: &mut Hub, event: NewEvent) -> Event {
    match HubProtocol::append(hub, event, 1001).unwrap() {
        AppendReceipt::Event(event) => *event, other => panic!("{other:?}"),
    }
}
fn approve(hub: &mut Hub, preview: &Preview) -> Event {
    append(hub, NewEvent { eid: preview.request_id.clone(), host: "test".into(), actor: "human:local".into(),
        kind: "main.resolution.approved".into(), layer: "L1".into(), obj: preview.target_commit.clone(),
        data: serde_json::to_value(preview).unwrap(), guards: vec![] })
}

#[test]
fn claim_requires_actual_human_original_and_does_not_create_a_baseline() {
    let (temp, anchor, target, mut hub, fence) = fixture(); let root = temp.path();
    let preview = preview(root, &target, &target, Operation::Claim);
    assert_eq!(preview.import_anchor, anchor);
    assert!(preview.diff.contains("-Original"));
    assert!(preview.diff.contains("+Edited manually"));
    let approval = approve(&mut hub, &preview);
    let resolution = main_resolution::prove_commit(root, &preview, &approval, &target).unwrap();
    let event = append(&mut hub, NewEvent {
        eid: events::derived_eid(&target, "main.change.resolved").unwrap(), host: "test".into(),
        actor: "process:towermd".into(), kind: "main.change.resolved".into(), layer: "L1".into(),
        obj: target.clone(), data: serde_json::to_value(&resolution).unwrap(), guards: vec![fence],
    });
    assert_eq!(main_resolution::prove_resolution(root, "resolution-project", "docs/tower", &event, &approval).unwrap(), resolution);
    assert_eq!(git(root, &["tag", "--list", "towermd/*"]), "");
    assert!(hub.events(0).iter().all(|event| !event.kind.starts_with("baseline.")));
    let mut forged = approval.clone(); forged.actor = "process:towermd-cli".into();
    assert!(main_resolution::prove_resolution(root, "resolution-project", "docs/tower", &event, &forged).is_err());
    assert!(main_resolution::prove_resolution(root, "another-project", "docs/tower", &event, &approval).is_err());
    let known = towermd_core::project_runtime::canonical_events(&hub.events(0)).unwrap();
    assert!(towermd_core::main_inspection::inspect(root, &target, "resolution-project", "docs/tower",
        None, "test", |eid| known.get(eid).cloned(), &towermd_devkit::dev_state::ASSEMBLY).passed);
    let mut projected = towermd_devkit::dev_state::initial();
    assert!(towermd_core::resolution_projection::apply(&mut projected, &approval).unwrap());
    assert!(towermd_core::resolution_projection::apply(&mut projected, &event).unwrap());
    assert!(projected["Baseline"].as_object().unwrap().is_empty());
    let alert_id = events::derived_eid(&target, "alert.raised.unapproved_protected_change").unwrap();
    projected["Alert"][&alert_id] = json!({"resolved":false});
    towermd_core::resolution_projection::reconcile_alerts(&mut projected).unwrap();
    assert_eq!(projected["Alert"][&alert_id]["resolved"], true);
    assert_eq!(projected["Alert"][&alert_id]["resolution_event"], event.eid);
    let mut tampered = preview.clone(); tampered.diff.push_str("\nUser did not see the original diff");
    tampered.content_hash = tampered.hash().unwrap();
    assert!(main_resolution::verify_preview(root, &tampered).is_err());
}

#[test]
fn sole_writer_revert_restores_exact_bytes_without_touching_user_worktree_or_index() {
    let (temp, _, target, mut hub, fence) = fixture(); let root = temp.path();
    let preview = preview(root, &target, &target, Operation::Revert);
    let approval = approve(&mut hub, &preview);
    let request = main_resolution::revert_request(root, &preview, &approval, fence.epoch).unwrap();
    assert!(main_writer::write(root, &request, || Ok(())).is_err());
    git(root, &["checkout", "--detach", "-q"]);
    write(root, "source.txt", "staged\n"); git(root, &["add", "source.txt"]); write(root, "source.txt", "unstaged\n");
    let index = fs::read(root.join(".git/index")).unwrap();
    let head = git(root, &["rev-parse", "HEAD"]);
    let resolved = match main_writer::write(root, &request, || Ok(())).unwrap() {
        WriteOutcome::Written { commit, tag: None, .. } => commit, other => panic!("{other:?}"),
    };
    let proof = main_resolution::prove_commit(root, &preview, &approval, &resolved).unwrap();
    assert_eq!(proof.operation, Operation::Revert);
    assert_eq!(proof.target_commit, target);
    assert_eq!(git(root, &["show", &format!("{resolved}:docs/tower/arch/A.md")]),
        "---\ntype: Module\nid: A\n---\nOriginal");
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read_to_string(root.join("source.txt")).unwrap(), "unstaged\n");
    assert_eq!(git(root, &["rev-parse", "HEAD"]), head);
    let accepted = append(&mut hub, NewEvent {
        eid: events::derived_eid(&target, "main.change.resolved").unwrap(), host: "test".into(),
        actor: "process:towermd".into(), kind: "main.change.resolved".into(), layer: "L1".into(),
        obj: target.clone(), data: serde_json::to_value(&proof).unwrap(), guards: vec![fence],
    });
    let known = towermd_core::project_runtime::canonical_events(&hub.events(0)).unwrap();
    let inspection = towermd_core::main_inspection::inspect(root, &resolved, "resolution-project", "docs/tower",
        None, "test", |eid| known.get(eid).cloned(), &towermd_devkit::dev_state::ASSEMBLY);
    assert!(inspection.complete && inspection.passed, "{:?}", inspection.blocked_commits);
    let mut forged_known = known.clone();
    forged_known.get_mut(&accepted.eid).unwrap().data["resolution_commit"] = json!(target);
    assert!(!towermd_core::main_inspection::inspect(root, &resolved, "resolution-project", "docs/tower",
        None, "test", |eid| forged_known.get(eid).cloned(), &towermd_devkit::dev_state::ASSEMBLY).passed);
    assert_eq!(main_resolution::find_revert(root, &resolved, &preview, &approval).unwrap(), Some(resolved.clone()));
    // Repeated confirmation finds the existing inverse, instead of reverting it.
    assert_eq!(main_resolution::find_revert(root, &resolved, &preview, &approval).unwrap(), Some(resolved));
}

#[test]
fn system_snapshots_configuration_baseline_manifests_and_untyped_protected_files_cannot_be_claimed() {
    for (path, body) in [
        ("docs/tower/state/state.json", "{}\n"),
        (".towermd/extra.yaml", "unregistered: true\n"),
        ("docs/tower/baselines/B0.md", "---\ntype: Baseline\n---\nForged\n"),
        ("outside/B0.md", "---\ntype: Baseline\nlayer: L1\n---\nForged\n"),
        ("docs/tower/arch/plain.md", "# Missing slow-object metadata\n"),
    ] {
        let (temp, _, _, _, _) = fixture(); let root = temp.path();
        write(root, path, body);
        let target = commit(root, "forbidden claim");
        for operation in [Operation::Claim, Operation::Revert] {
            assert!(main_resolution::prepare(root, &target, "resolution-project", "docs/tower", &target,
                operation, &uuid::Uuid::now_v7().to_string()).is_err(), "{path}");
        }
    }
}

#[test]
fn changed_layer_and_later_conflicts_are_checked_before_and_after_the_target() {
    let (temp, _, _, _, _) = fixture(); let root = temp.path();
    write(root, "docs/tower/arch/A.md", "---\ntype: Module\nid: A\nlayer: L4\n---\nDowngraded\n");
    let target = commit(root, "layer change");
    assert!(main_resolution::prepare(root, &target, "resolution-project", "docs/tower", &target,
        Operation::Claim, &uuid::Uuid::now_v7().to_string()).is_ok());
    write(root, "docs/tower/arch/A.md", "---\ntype: Module\nid: A\nlayer: L4\n---\nLater unrelated edit\n");
    let main = commit(root, "later edit");
    assert!(main_resolution::prepare(root, &main, "resolution-project", "docs/tower", &target,
        Operation::Revert, &uuid::Uuid::now_v7().to_string()).unwrap_err().contains("同一路径"));
    write(root, "docs/tower/arch/A.md", "---\ntype: Module\nlayer: [L1]\n---\nInvalid layer\n");
    let invalid = commit(root, "invalid layer");
    assert!(main_resolution::prepare(root, &invalid, "resolution-project", "docs/tower", &invalid,
        Operation::Claim, &uuid::Uuid::now_v7().to_string()).is_err());
}

#[test]
fn executable_mode_and_forged_revert_contents_cannot_be_silently_normalized_or_certified() {
    let (temp, _, target, mut hub, fence) = fixture(); let root = temp.path();
    let preview = preview(root, &target, &target, Operation::Revert);
    let approval = approve(&mut hub, &preview);
    git(root, &["checkout", "--detach", "-q"]);
    let mut request = main_resolution::revert_request(root, &preview, &approval, fence.epoch).unwrap();
    request.changes.insert("docs/tower/arch/A.md".into(), Some(b"---\ntype: Module\n---\nNot approved\n".to_vec()));
    let forged = match main_writer::write(root, &request, || Ok(())).unwrap() {
        WriteOutcome::Written { commit, .. } => commit, other => panic!("{other:?}"),
    };
    assert!(main_resolution::prove_commit(root, &preview, &approval, &forged).is_err());
    git(root, &["checkout", "-q", "main"]);
    git(root, &["update-index", "--chmod=+x", "docs/tower/arch/A.md"]);
    git(root, &["commit", "-qm", "change executable mode"]);
    let executable = git(root, &["rev-parse", "HEAD"]);
    assert!(main_resolution::prepare(root, &executable, "resolution-project", "docs/tower", &executable,
        Operation::Revert, &uuid::Uuid::now_v7().to_string()).unwrap_err().contains("100755"));
    // Arbitrary JSON cannot replace a canonical approval.
    let mut fake = approval; fake.data = json!({"approved":true});
    assert!(main_resolution::from_approval(&fake).is_err());
}
