use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use towermd_core::durable_outbox::DurableOutbox;
use towermd_core::hub::Hub;
use towermd_core::hub_protocol::*;
use towermd_core::main_writer::{self, WriteKind, WriteRequest};
use towermd_core::project_runtime::{
    self, ProjectRuntime, RecoveryEvidence, RuntimeConfig, RuntimeError,
};
use towermd_core::snapshot::Snapshot;

static REDUCE_CALLS: AtomicUsize = AtomicUsize::new(0);

fn reduce(value: &mut Value, event: &Event) -> Result<(), String> {
    if matches!(
        event.kind.as_str(),
        "lease.expired" | "lease.released" | "recovery.completed" | "hub.rebuild.confirmed" | "snapshot.written"
    ) {
        return Ok(());
    }
    if event.kind != "task.created" {
        return Err(format!("unexpected business event {}", event.kind));
    }
    if event.data["count_calls"] == true {
        REDUCE_CALLS.fetch_add(1, Ordering::SeqCst);
    }
    value
        .as_array_mut()
        .ok_or("array required")?
        .push(json!(event.obj));
    Ok(())
}
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
            "user.name=Runtime Test",
            "-c",
            "user.email=qa@localhost",
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
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}
fn fixture() -> (tempfile::TempDir, RuntimeConfig) {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-qb", "main"]);
    fs::write(temp.path().join("source.txt"), "original\n").unwrap();
    fs::create_dir(temp.path().join(".towermd")).unwrap();
    fs::write(temp.path().join(".towermd/config.yaml"),
        "project_id: runtime-project\ndocs_dir: docs/tower\nhub: local\n").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "initial"]);
    let config = RuntimeConfig {
        root: temp.path().to_owned(),
        common_dir: temp.path().join(".git"),
        project_id: "runtime-project".into(),
        docs_dir: "docs/tower".into(),
        host: "local".into(),
    };
    (temp, config)
}
fn input(runtime: &ProjectRuntime, obj: &str) -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "local".into(),
        actor: "process:towermd-cli".into(),
        kind: "task.created".into(),
        layer: "L4".into(),
        obj: obj.into(),
        data: json!({}),
        guards: vec![runtime.main_fence()],
    }
}
fn no_trailers(_: RecoveryEvidence<'_>) -> Result<Vec<NewEvent>, String> {
    Ok(vec![])
}

#[test]
fn pause_is_recovered_from_confirmed_originals_then_survives_a_second_rebuild_without_outbox() {
    use towermd_core::{project_controls,project_state};
    let (_temp,config)=fixture();
    let initial=project_state::core_initial();
    let mut runtime=ProjectRuntime::open(config.clone(),initial.clone(),project_state::core_apply,||1000).unwrap();
    let pause_id=uuid::Uuid::now_v7().to_string();
    let pause=project_controls::prepare_pause(runtime.projection(),&pause_id, &[]).unwrap();
    let original=runtime.append(NewEvent{eid:pause_id.clone(),host:"local".into(),actor:"human:local".into(),
        kind:"project.paused".into(),layer:"L5".into(),obj:config.project_id.clone(),
        data:serde_json::to_value(pause).unwrap(),guards:vec![]},||1001).unwrap();
    assert!(runtime.snapshot_commit().is_none());
    assert!(runtime.acquire_work_leases("R-blocked",vec!["module:A-one".into()],30_000,||1002).is_err());
    drop(runtime);
    fs::remove_dir_all(config.common_dir.join("towermd/hub")).unwrap();
    let mut runtime=ProjectRuntime::recover(config.clone(),initial.clone(),project_state::core_apply,||2000,no_trailers).unwrap();
    assert_eq!(runtime.receipt(&pause_id),Some(&original));
    assert!(project_controls::read(runtime.projection()).unwrap().paused);
    assert!(runtime.acquire_work_leases("R-blocked",vec!["module:A-one".into()],30_000,||2001).is_err());
    runtime.snapshot(||2002).unwrap();
    drop(runtime);
    fs::remove_dir_all(config.common_dir.join("towermd/hub")).unwrap();
    fs::remove_dir_all(config.common_dir.join("towermd/outbox")).unwrap();
    let mut runtime=ProjectRuntime::recover(config.clone(),initial.clone(),project_state::core_apply,||3000,no_trailers).unwrap();
    assert_eq!(runtime.receipt(&pause_id),Some(&original));
    assert!(runtime.acquire_work_leases("R-blocked",vec!["module:A-one".into()],30_000,||3001).is_err());
    let resume_id=uuid::Uuid::now_v7().to_string();
    let resume=project_controls::prepare_resume(runtime.projection(),&resume_id,pause_id.clone(), &[]).unwrap();
    runtime.append(NewEvent{eid:resume_id,host:"local".into(),actor:"human:local".into(),
        kind:"project.resumed".into(),layer:"L5".into(),obj:pause_id,data:serde_json::to_value(resume).unwrap(),guards:vec![]},||3002).unwrap();
    assert!(!project_controls::read(runtime.projection()).unwrap().paused);
    runtime.acquire_work_leases("R-allowed",vec!["module:A-one".into()],30_000,||3003).unwrap();
    runtime.snapshot(||3004).unwrap();
    drop(runtime);
    let runtime=ProjectRuntime::open(config,initial,project_state::core_apply,||3005).unwrap();
    assert!(!project_controls::read(runtime.projection()).unwrap().paused);
}

#[test]
fn activation_keeps_reviewed_main_unchanged_and_appends_reduce_only_new_events() {
    let (temp, config) = fixture();
    let before = git(temp.path(), &["rev-parse", "HEAD"]);
    let index = fs::read(temp.path().join(".git/index")).unwrap();
    let mut runtime = ProjectRuntime::open(config, json!([]), reduce, || 1000).unwrap();
    assert_eq!(runtime.contiguous_seq(), 1);
    assert!(runtime.snapshot_commit().is_none());
    assert!(main_writer::latest_snapshot(temp.path()).unwrap().is_none());
    assert!(temp.path().join(".git/towermd/runtime.json").is_file());
    assert!(!temp.path().join("docs").exists());
    REDUCE_CALLS.store(0, Ordering::SeqCst);
    for i in 0..12 {
        let mut event = input(&runtime, &format!("T-{i}"));
        event.data["count_calls"] = json!(true);
        runtime.append(event, || 1001 + i).unwrap();
    }
    // One pure preflight plus one actual apply per event; no old-event replay.
    assert_eq!(REDUCE_CALLS.load(Ordering::SeqCst), 24);
    assert_eq!(runtime.projection().as_array().unwrap().len(), 12);
    assert_eq!(before, git(temp.path(), &["rev-parse", "HEAD"]));
    assert_eq!(index, fs::read(temp.path().join(".git/index")).unwrap());
}

#[test]
fn main_lease_renews_at_thirty_seconds_and_expiry_fences_mutations() {
    let (_temp, config) = fixture();
    let mut runtime = ProjectRuntime::open(config, json!([]), reduce, || 1000).unwrap();
    let fence = runtime.main_fence();
    runtime.tick(|| 30_999).unwrap();
    assert_eq!(runtime.contiguous_seq(), 1);
    runtime.tick(|| 31_000).unwrap();
    assert_eq!(runtime.contiguous_seq(), 2);
    assert_eq!(runtime.main_fence(), fence);
    runtime.snapshot(|| 31_001).unwrap();
    let event = input(&runtime, "T-expired");
    assert!(matches!(
        runtime.append(event, || 151_001),
        Err(RuntimeError::Fenced(_))
    ));
    assert_eq!(runtime.projection(), &json!([]));
}

#[test]
fn snapshot_and_restart_keep_original_receipt_and_do_not_duplicate_projection() {
    let (_temp, config) = fixture();
    let mut runtime = ProjectRuntime::open(config.clone(), json!([]), reduce, || 1000).unwrap();
    let event = runtime.append(input(&runtime, "T-a"), || 1001).unwrap();
    runtime.snapshot(|| 1002).unwrap();
    drop(runtime);
    let mut runtime = ProjectRuntime::open(config, json!([]), reduce, || 1003).unwrap();
    assert_eq!(runtime.projection(), &json!(["T-a"]));
    assert_eq!(runtime.receipt(&event.eid), Some(&event));
    runtime.snapshot(|| 1004).unwrap();
    assert_eq!(runtime.projection(), &json!(["T-a"]));
}

#[test]
fn business_events_do_not_require_main_lease_and_invalid_transitions_never_enter_hub() {
    let (_temp, config) = fixture();
    let mut runtime = ProjectRuntime::open(config, json!([]), reduce, || 1000).unwrap();
    let mut invalid = input(&runtime, "T-invalid");
    invalid.kind = "unsupported.transition".into();
    let before = runtime.contiguous_seq();
    assert!(matches!(
        runtime.append(invalid, || 1001),
        Err(RuntimeError::Invalid(_))
    ));
    assert_eq!(runtime.contiguous_seq(), before);
    let mut ordinary = input(&runtime, "T-ordinary");
    ordinary.guards.clear();
    let event = runtime.append(ordinary, || 121_001).unwrap();
    assert!(!event.stale);
    assert_eq!(runtime.projection(), &json!(["T-ordinary"]));
}

#[test]
fn confirmed_retry_returns_original_after_main_lease_changes() {
    let (_temp, config) = fixture();
    let mut runtime = ProjectRuntime::open(config.clone(), json!([]), reduce, || 1000).unwrap();
    let request = input(&runtime, "T-a");
    let original = runtime.append(request.clone(), || 1001).unwrap();
    runtime.snapshot(|| 1002).unwrap();
    drop(runtime);
    let mut runtime = ProjectRuntime::open(config, json!([]), reduce, || 200_000).unwrap();
    assert_ne!(runtime.main_fence().epoch, request.guards[0].epoch);
    let before = runtime.contiguous_seq();
    assert_eq!(runtime.append(request, || 200_001).unwrap(), original);
    assert_eq!(runtime.contiguous_seq(), before);
    assert_eq!(runtime.projection(), &json!(["T-a"]));
}

#[test]
fn git_publication_before_ack_is_completed_on_reopen() {
    let (_temp, config) = fixture();
    let mut runtime = ProjectRuntime::open(config.clone(), json!([]), reduce, || 1000).unwrap();
    runtime.snapshot(|| 1000).unwrap();
    runtime.append(input(&runtime, "T-a"), || 1001).unwrap();
    let old_commit = runtime.snapshot_commit().unwrap().to_owned();
    let epoch = runtime.main_fence().epoch;
    drop(runtime);
    let previous = Snapshot::read(
        &config.root,
        &old_commit,
        &config.docs_dir,
        &config.project_id,
        |events| project_runtime::replay(&json!([]), events, reduce),
    )
    .unwrap();
    let hub = Hub::open_existing(&config.common_dir.join("towermd/hub")).unwrap();
    let next = Snapshot::build(
        &config.project_id,
        Some(&previous),
        &hub.events(previous.state.seq),
        hub.last_seq(),
        epoch,
        hub.epoch_high_water(),
        |events| project_runtime::replay(&json!([]), events, reduce),
    )
    .unwrap();
    let result = main_writer::write(
        &config.root,
        &WriteRequest {
            docs_dir: config.docs_dir.clone(),
            epoch,
            kind: WriteKind::Snapshot {
                seq: next.state.seq,
                promote_from: None,
            },
            changes: next.files(&config.docs_dir).unwrap(),
            message: "Crash before ack".into(),
        },
        || Ok(()),
    )
    .unwrap();
    let commit = match result {
        main_writer::WriteOutcome::Written { commit, .. } => commit,
        _ => panic!("expected new commit"),
    };
    assert_ne!(hub.snapshot_ack().unwrap().commit, commit);
    drop(hub);
    let runtime = ProjectRuntime::open(config.clone(), json!([]), reduce, || 1002).unwrap();
    assert_eq!(runtime.snapshot_commit(), Some(commit.as_str()));
    assert_eq!(runtime.projection(), &json!(["T-a"]));
    drop(runtime);
    assert_eq!(
        Hub::open_existing(&config.common_dir.join("towermd/hub"))
            .unwrap()
            .snapshot_ack()
            .unwrap()
            .commit,
        commit
    );
}

#[test]
fn activated_missing_or_corrupt_hub_never_silently_starts_empty() {
    for corrupt in [false, true] {
        let (_temp, config) = fixture();
        let runtime = ProjectRuntime::open(config.clone(), json!([]), reduce, || 1000).unwrap();
        drop(runtime);
        let hub = config.common_dir.join("towermd/hub");
        if corrupt {
            fs::write(hub.join("journal.jsonl"), b"not-json\n").unwrap();
        } else {
            fs::remove_dir_all(&hub).unwrap();
        }
        assert!(matches!(
            ProjectRuntime::open(config, json!([]), reduce, || 1001),
            Err(RuntimeError::NeedsRecovery(_))
        ));
        if corrupt {
            assert_eq!(fs::read(hub.join("journal.jsonl")).unwrap(), b"not-json\n");
        } else {
            assert!(!hub.exists());
        }
    }
}

#[test]
fn recovery_preserves_unsnapshotted_confirmed_original_and_second_recovery_needs_no_outbox() {
    let (_temp, config) = fixture();
    let mut runtime = ProjectRuntime::open(config.clone(), json!([]), reduce, || 1000).unwrap();
    let original = runtime.append(input(&runtime, "T-a"), || 1001).unwrap();
    let old_fence = runtime.main_fence();
    drop(runtime);
    let outbox_dir = config.common_dir.join("towermd/outbox");
    let confirmed_dir = outbox_dir.join("confirmed");
    let receipt_path = fs::read_dir(&confirmed_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let original_bytes = fs::read(&receipt_path).unwrap();
    fs::remove_dir_all(config.common_dir.join("towermd/hub")).unwrap();
    let runtime =
        ProjectRuntime::recover(config.clone(), json!([]), reduce, || 2000, no_trailers).unwrap();
    assert_eq!(runtime.projection(), &json!(["T-a"]));
    assert_eq!(runtime.receipt(&original.eid), Some(&original));
    assert!(runtime.main_fence().epoch >= old_fence.epoch + (1 << 32));
    assert!(runtime.contiguous_seq() >= original.seq + (1 << 32));
    assert_eq!(fs::read(&receipt_path).unwrap(), original_bytes);
    let first_seq = runtime.contiguous_seq();
    let first_epoch = runtime.main_fence().epoch;
    drop(runtime);
    fs::remove_dir_all(config.common_dir.join("towermd/hub")).unwrap();
    fs::remove_dir_all(outbox_dir).unwrap();
    let runtime =
        ProjectRuntime::recover(config.clone(), json!([]), reduce, || 3000, no_trailers).unwrap();
    assert_eq!(runtime.projection(), &json!(["T-a"]));
    assert_eq!(runtime.receipt(&original.eid), Some(&original));
    assert!(runtime.contiguous_seq() >= first_seq + (1 << 32));
    assert!(runtime.main_fence().epoch >= first_epoch + (1 << 32));
    drop(runtime);
    let outbox = DurableOutbox::open(&config.common_dir).unwrap();
    assert!(outbox.confirmed().unwrap().iter().all(|event| event.kind == "snapshot.written"));
    assert!(outbox.confirmed_by_eid(&original.eid).unwrap().is_none());
}

struct Offline;
impl HubProtocol for Offline {
    fn append(&mut self, _: NewEvent, _: u64) -> Result<AppendReceipt, HubError> {
        Err(HubError::Uncertain("offline".into()))
    }
    fn subscribe(&mut self, _: u64, _: u64) -> Result<SubscriptionBatch, HubError> {
        unreachable!()
    }
    fn lease_acquire(&mut self, _: AcquireLease, _: u64) -> Result<LeaseReceipt, HubError> {
        unreachable!()
    }
    fn lease_renew(&mut self, _: RenewLease, _: u64) -> Result<LeaseReceipt, HubError> {
        unreachable!()
    }
    fn lease_release(&mut self, _: ReleaseLease, _: u64) -> Result<LeaseReceipt, HubError> {
        unreachable!()
    }
    fn snapshot_ack(&mut self, _: SnapshotAck, _: Value) -> Result<(), HubError> {
        unreachable!()
    }
    fn rebuild(&mut self, _: RebuildSeed) -> Result<RebuildReceipt, HubError> {
        unreachable!()
    }
    fn contiguous_seq(&self) -> u64 {
        0
    }
}

#[test]
fn pending_old_fence_replays_as_stale_and_never_certifies_action() {
    let (_temp, config) = fixture();
    let runtime = ProjectRuntime::open(config.clone(), json!([]), reduce, || 1000).unwrap();
    let pending = input(&runtime, "T-not-accepted");
    drop(runtime);
    let mut outbox = DurableOutbox::open(&config.common_dir).unwrap();
    assert!(outbox
        .deliver(&mut Offline, pending.clone(), 1001, |_, _, _| Ok(None))
        .is_err());
    assert!(outbox.confirmed().unwrap().is_empty());
    drop(outbox);
    fs::remove_dir_all(config.common_dir.join("towermd/hub")).unwrap();
    let runtime =
        ProjectRuntime::recover(config.clone(), json!([]), reduce, || 2000, no_trailers).unwrap();
    assert_eq!(runtime.projection(), &json!([]));
    assert!(runtime.receipt(&pending.eid).unwrap().stale);
    assert!(runtime.pending().unwrap().is_empty());
}

#[test]
fn ordinary_producer_cannot_inject_recovery_wrapper_and_unproved_recovery_stops() {
    let (_temp, config) = fixture();
    let mut runtime = ProjectRuntime::open(config.clone(), json!([]), reduce, || 1000).unwrap();
    let mut forged = input(&runtime, "T-forged");
    forged.kind = "recovery.confirmed".into();
    let before = runtime.contiguous_seq();
    assert!(matches!(
        runtime.append(forged, || 1001),
        Err(RuntimeError::Invalid(_))
    ));
    assert_eq!(runtime.contiguous_seq(), before);
    runtime.snapshot(|| 1001).unwrap();
    drop(runtime);
    let snapshot = main_writer::latest_snapshot(&config.root).unwrap().unwrap();
    assert!(matches!(
        ProjectRuntime::recover(
            config.clone(),
            json!([]),
            reduce,
            || 2000,
            |_| Err("main proof unavailable".into())
        ),
        Err(RuntimeError::NeedsRecovery(_))
    ));
    assert_eq!(
        main_writer::latest_snapshot(&config.root).unwrap().unwrap(),
        snapshot
    );
    let runtime = ProjectRuntime::open(config, json!([]), reduce, || 2001).unwrap();
    assert_eq!(runtime.projection(), &json!([]));
}

#[test]
fn periodic_snapshot_ignores_renewals_and_writes_only_when_dirty() {
    let (_temp, config) = fixture();
    let mut runtime = ProjectRuntime::open(config.clone(), json!([]), reduce, || 1000).unwrap();
    let original = runtime.snapshot(|| 1000).unwrap();
    for step in 1..=60 {
        runtime.tick(|| 1000 + step * 30_000).unwrap();
    }
    assert!(!runtime.snapshot_due(1_801_000));
    assert_eq!(runtime.snapshot(|| 1_801_000).unwrap(), original);
    assert_eq!(
        main_writer::latest_snapshot(&config.root)
            .unwrap()
            .unwrap()
            .commit,
        original
    );
    runtime
        .append(input(&runtime, "T-new"), || 1_801_001)
        .unwrap();
    assert!(runtime.snapshot_due(1_801_001));
    let next = runtime.snapshot(|| 1_801_002).unwrap();
    assert_ne!(next, original);
    assert!(!runtime.snapshot_due(1_801_002));
    assert!(!runtime.snapshot_due(3_601_003));
}

#[test]
fn snapshot_receipt_is_durable_and_private_state_is_published_when_main_becomes_writable() {
    let (temp, config)=fixture();
    let mut runtime=ProjectRuntime::open(config.clone(),json!([]),reduce,||1000).unwrap();
    runtime.append(input(&runtime,"T-durable"),||1001).unwrap();
    let private=runtime.snapshot(||1002).unwrap();
    let eid=towermd_core::events::derived_eid(&private,"snapshot.written").unwrap();
    let receipt=runtime.receipt(&eid).unwrap().clone();
    assert_eq!(receipt.actor,"process:towermd-snapshot");
    assert_eq!(receipt.data["commit"],private);
    assert!(receipt.seq>receipt.data["seq"].as_u64().unwrap());
    assert_eq!(runtime.snapshot(||1003).unwrap(),private);
    assert_eq!(runtime.receipt(&eid),Some(&receipt));
    git(temp.path(),&["checkout","-qb","qa-working"]);
    let head=git(temp.path(),&["rev-parse","HEAD"]);
    let index=fs::read(temp.path().join(".git/index")).unwrap();
    let published=runtime.snapshot(||1004).unwrap();
    assert_ne!(published,private);
    assert_eq!(git(temp.path(),&["rev-parse","main"]),published);
    assert_eq!(git(temp.path(),&["rev-parse","refs/towermd/snapshot"]),private);
    assert_eq!(git(temp.path(),&["rev-parse","HEAD"]),head);
    assert_eq!(fs::read(temp.path().join(".git/index")).unwrap(),index);
    assert_eq!(runtime.projection(),&json!(["T-durable"]));
    let seq=runtime.contiguous_seq();
    assert_eq!(runtime.snapshot(||1005).unwrap(),published);
    assert_eq!(runtime.contiguous_seq(),seq);
    drop(runtime);
    let outbox=DurableOutbox::open(&config.common_dir).unwrap();
    assert_eq!(outbox.confirmed_by_eid(&eid).unwrap(),Some(receipt));
}

fn reduce_lease_end(value: &mut Value, event: &Event) -> Result<(), String> {
    if !matches!(event.kind.as_str(), "lease.expired" | "lease.released") {
        return Err("unexpected event".into());
    }
    value.as_array_mut().ok_or("array expected")?.push(json!({
        "kind":event.kind,"actor":event.actor,"names":event.released_lease_names(),
    }));
    Ok(())
}

#[test]
fn module_leases_are_atomic_and_actual_termination_events_reach_the_reducer() {
    let (_temp, config) = fixture();
    let mut runtime = ProjectRuntime::open(config, json!([]), reduce_lease_end, || 1000).unwrap();
    let leases = runtime
        .acquire_work_leases(
            "run-a",
            vec!["module:a".into(), "module:b".into()],
            900_000,
            || 1001,
        )
        .unwrap();
    assert_eq!(leases.len(), 2);
    assert!(runtime
        .acquire_work_leases(
            "run-b",
            vec!["module:b".into(), "module:c".into()],
            900_000,
            || 1002
        )
        .is_err());
    assert_eq!(runtime.work_leases(1002).len(), 2);
    assert!(!runtime.work_leases(1002).contains_key("module:c"));
    let fences: Vec<_> = leases.iter().map(Lease::fence).collect();
    let renewed = runtime
        .renew_work_leases(fences.clone(), 900_000, || 1003)
        .unwrap();
    assert_eq!(renewed[0].epoch, leases[0].epoch);
    runtime
        .release_work_leases(vec![fences[0].clone()], || 1004)
        .unwrap();
    assert_eq!(
        runtime.projection()[0],
        json!({
            "kind":"lease.released","actor":"process:towermd-hub","names":["module:a"],
        })
    );
    let before = runtime.contiguous_seq();
    assert!(runtime
        .release_work_leases(vec![runtime.main_fence()], || 1005)
        .is_err());
    assert_eq!(runtime.contiguous_seq(), before);
    assert!(matches!(
        runtime.tick(|| 901_004),
        Err(RuntimeError::Fenced(_))
    ));
    assert!(runtime
        .projection()
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "lease.expired"
            && event["names"]
                .as_array()
                .unwrap()
                .contains(&json!("module:b"))));
    assert!(runtime.work_leases(901_004).is_empty());
    assert!(runtime
        .renew_work_leases(vec![fences[1].clone()], 900_000, || 901_005)
        .is_err());
    let replacement = runtime
        .acquire_work_leases("run-new", vec!["module:b".into()], 900_000, || 901_006)
        .unwrap();
    assert!(replacement[0].epoch > fences[1].epoch);
}

fn confirmation(config: &RuntimeConfig) -> NewEvent {
    let eid = uuid::Uuid::now_v7().to_string();
    NewEvent {
        eid: eid.clone(),
        host: config.host.clone(),
        actor: "human:local".into(),
        kind: "hub.rebuild.confirmed".into(),
        layer: "L5".into(),
        obj: config.project_id.clone(),
        data: json!({"request_id":eid}),
        guards: vec![],
    }
}

#[test]
fn human_confirmation_is_in_the_first_recovery_boundary_and_retry_does_not_jump_again() {
    let (_temp, config) = fixture();
    let mut runtime = ProjectRuntime::open(config.clone(), json!([]), reduce, || 1000).unwrap();
    runtime
        .append(input(&runtime, "T-before-first-snapshot"), || 1001)
        .unwrap();
    assert!(runtime.snapshot_commit().is_none());
    drop(runtime);
    fs::remove_dir_all(config.common_dir.join("towermd/hub")).unwrap();
    let request = confirmation(&config);
    let runtime = ProjectRuntime::recover_confirmed(
        config.clone(),
        json!([]),
        reduce,
        request.clone(),
        || 2000,
        no_trailers,
    )
    .unwrap();
    let original = runtime.receipt(&request.eid).unwrap().clone();
    assert_eq!(original.actor, "human:local");
    assert_eq!(runtime.projection(), &json!(["T-before-first-snapshot"]));
    let commit = runtime.snapshot_commit().unwrap().to_owned();
    let snapshot = Snapshot::read(
        &config.root,
        &commit,
        &config.docs_dir,
        &config.project_id,
        |events| project_runtime::replay(&json!([]), events, reduce),
    )
    .unwrap();
    assert!(snapshot.events.iter().any(|event| event == &original));
    assert_eq!(snapshot.state.recovery_jumps.len(), 1);
    drop(runtime);
    let runtime = ProjectRuntime::recover_confirmed(
        config.clone(),
        json!([]),
        reduce,
        request,
        || 2001,
        |_| panic!("accepted confirmation must not invoke proof or jump again"),
    )
    .unwrap();
    assert_eq!(runtime.snapshot_commit(), Some(commit.as_str()));
    assert_eq!(runtime.receipt(&original.eid), Some(&original));
}

#[test]
fn confirmed_retry_installs_the_original_checked_candidate_after_git_before_install_failure() {
    let (_temp, config) = fixture();
    let runtime = ProjectRuntime::open(config.clone(), json!([]), reduce, || 1000).unwrap();
    drop(runtime);
    fs::remove_dir_all(config.common_dir.join("towermd/hub")).unwrap();
    let request = confirmation(&config);
    let runtime = ProjectRuntime::recover_confirmed(
        config.clone(),
        json!([]),
        reduce,
        request.clone(),
        || 2000,
        no_trailers,
    )
    .unwrap();
    let commit = runtime.snapshot_commit().unwrap().to_owned();
    drop(runtime);
    // The simulated failure is before candidate installation, therefore the
    // later successful-install snapshot notification could not yet exist.
    let outbox = DurableOutbox::open(&config.common_dir).unwrap();
    assert!(outbox.confirmed().unwrap().iter().all(|event| event.kind == "snapshot.written"));
    drop(outbox);
    fs::remove_dir_all(config.common_dir.join("towermd/outbox")).unwrap();
    let snapshot = Snapshot::read(
        &config.root,
        &commit,
        &config.docs_dir,
        &config.project_id,
        |events| project_runtime::replay(&json!([]), events, reduce),
    )
    .unwrap();
    let jump = snapshot.state.recovery_jumps.last().unwrap();
    // Recreate the exact pre-install candidate through the real Hub protocol.
    let candidate_path = config
        .common_dir
        .join(format!("towermd/recovery-{}", uuid::Uuid::now_v7()));
    let mut candidate = Hub::open(&candidate_path).unwrap();
    candidate
        .rebuild(RebuildSeed {
            pause_event: None,
            source_seq: jump.seq_boundary - (1 << 32),
            source_epoch: jump.epoch_boundary - (1 << 32),
            seq_boundary: jump.seq_boundary,
            epoch_boundary: jump.epoch_boundary,
            checkpoint: json!({}),
            receipts: snapshot
                .receipts()
                .into_iter()
                .filter(|(_, seq)| *seq < jump.seq_boundary)
                .collect(),
            tail: snapshot
                .events
                .iter()
                .filter(|event| event.seq > jump.seq_boundary)
                .cloned()
                .collect(),
            final_snapshot: Some(RebuildSnapshot {
                ack: SnapshotAck {
                    commit: commit.clone(),
                    seq: snapshot.state.seq,
                    epoch: snapshot.state.epoch,
                },
                checkpoint: serde_json::to_value(&snapshot.state).unwrap(),
            }),
        })
        .unwrap();
    drop(candidate);
    fs::remove_dir_all(config.common_dir.join("towermd/hub")).unwrap();
    let runtime = ProjectRuntime::recover_confirmed(
        config.clone(),
        json!([]),
        reduce,
        request.clone(),
        || 2001,
        |_| panic!("resume does not approve a new boundary"),
    )
    .unwrap();
    assert_eq!(runtime.snapshot_commit(), Some(commit.as_str()));
    assert!(runtime.receipt(&request.eid).is_some());
    assert!(!candidate_path.exists());
    drop(runtime);
    fs::remove_dir_all(config.common_dir.join("towermd/hub")).unwrap();
    assert!(matches!(
        ProjectRuntime::recover_confirmed(
            config.clone(),
            json!([]),
            reduce,
            request,
            || 2002,
            no_trailers
        ),
        Err(RuntimeError::NeedsRecovery(_))
    ));
    assert_eq!(
        main_writer::latest_snapshot(&config.root)
            .unwrap()
            .unwrap()
            .commit,
        commit
    );
}
