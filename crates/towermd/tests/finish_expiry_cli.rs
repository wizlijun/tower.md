//! Actual CLI reads a real core-produced fixed-deadline expiry. No raw Event,
//! Run, report, Job JSON or expiry ticket is manufactured by this adapter test.
#[allow(dead_code)]
#[path = "support/finish_cli_fixture.rs"]
mod cli_fixture;
#[allow(dead_code)]
#[path = "support/finish_fixture.rs"]
mod support;
use cli_fixture::Fixture;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    sync::Mutex,
    time::{Duration, Instant},
};
use towermd_core::{
    catalog::Catalog,
    finish::FinishStart,
    hub::Fence,
    project_session::ProjectSession,
    recovery_outbox::{RecoveryContext, RecoveryOutbox},
};
fn now() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}
#[test]
fn expired_original_finish_cli_is_nonzero_history_without_execution_and_cannot_change_new_run_epoch(
) {
    let f = Fixture::new(&["printf x >> \"$TOWERMD_RUNTIME_DIR/expired-must-not-execute\""]);
    f.success(&["daemon", "stop"]);
    support::git(f.root.path(), &["rebase", "main"]);
    let catalog = Catalog::load(f.root.path()).unwrap();
    let mut outbox = RecoveryOutbox::open(
        &f.runtime
            .path()
            .join("recovery-outbox")
            .join(catalog.config.project_id()),
        RecoveryContext::from_catalog(&catalog).unwrap(),
    )
    .unwrap();
    let open_deadline = Instant::now() + Duration::from_secs(15);
    let session = loop {
        match ProjectSession::open(
            &catalog.common_dir.join("towermd"),
            &catalog,
            &outbox.read_inputs().unwrap(),
            now(),
        ) {
            Ok(session) => break Mutex::new(session),
            Err(error) => {
                assert!(
                    error.to_string().contains("lock") || error.to_string().contains("锁"),
                    "{error}"
                );
                assert!(
                    Instant::now() < open_deadline,
                    "previous daemon did not release actual Hub ownership"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };
    let request = uuid::Uuid::now_v7().to_string();
    let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &session,
        &catalog,
        "host",
        &request,
        None,
        &mut outbox,
        || Ok(now()),
    )
    .unwrap() else {
        panic!("real first admission must be queued")
    };
    let status = ticket.status().clone();
    drop(ticket); // No ExecutionTicket exists, hence no owned child to cancel.
    let original_start = outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .find(|event| event.eid == status.started_eid)
        .unwrap()
        .clone();
    // Simulate the real daemon's fixed-deadline observation in core,
    // then reopen the binary from actual durable history. No client deadline
    // field exists and no raw expiry body is accepted by any public API.
    let deadline = status.input.merge_deadline;
    let maintenance = ProjectSession::maintain_finishes_shared(
        &session,
        &catalog,
        "host",
        &BTreeSet::new(),
        None,
        &mut outbox,
        || Ok(deadline),
    )
    .unwrap();
    assert!(maintenance.renewed.is_empty());
    assert!(maintenance.expired.is_empty());
    assert_eq!(maintenance.expirations.len(), 1);
    let expiry = maintenance.expirations.into_iter().next().unwrap();
    assert_eq!(expiry.run_id(), status.input.run_id);
    assert_eq!(expiry.deadline(), deadline);
    assert_eq!(
        expiry.evidence_ids(),
        std::slice::from_ref(&status.input.evidence_id)
    );
    let expired = ProjectSession::expire_finish_shared(
        &session,
        &catalog,
        "host",
        expiry,
        &mut outbox,
        || Ok(deadline),
    )
    .unwrap()
    .expect("unmerged actual queued job must expire");
    assert!(!expired.actual.stale);
    assert_eq!(expired.actual.data["run"]["state"], "expired");
    assert_eq!(expired.actual.data["task"]["state"], "ready");
    assert_eq!(
        expired.actual.data["run"]["finish"]["jobs"][&status.input.evidence_id]["input"],
        serde_json::to_value(&status.input).unwrap()
    );
    drop(session);
    drop(outbox);
    let args = [
        "--run",
        status.input.run_id.as_str(),
        "finish",
        "--wait",
        status.input.evidence_id.as_str(),
    ];
    let first = f.output_before(&args, Instant::now() + Duration::from_secs(90));
    assert!(
        !first.status.success(),
        "expired is not a new successful finish admission"
    );
    assert!(
        String::from_utf8_lossy(&first.stderr).contains("验证已到固定截止"),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let historical: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(historical["evidence_id"], status.input.evidence_id);
    assert_eq!(historical["job_state"], "expired");
    assert_eq!(historical["run_state"], "expired");
    assert_eq!(
        historical["input"],
        serde_json::to_value(&status.input).unwrap()
    );
    assert!(historical.get("result").is_none());
    assert!(historical.get("evidence").is_none());
    assert!(!f.runtime.path().join("expired-must-not-execute").exists());
    // Persist the actual expiry before the next single claim, so a pending
    // background snapshot of that prior fact is not its source precondition.
    f.snapshot_before_finish_expiry(&expired.actual);
    f.json(&["task", "claim", "T-finish"]);
    let before = f.outbox();
    let new_claim = before
        .confirmed()
        .iter()
        .find(|event| {
            event.kind == "task.claimed"
                && event.data["task"]["id"] == "T-finish"
                && event.data["run"]["id"] != status.input.run_id
        })
        .unwrap()
        .clone();
    let new_run = new_claim.data["run"]["id"].as_str().unwrap();
    let new_fences: Vec<Fence> =
        serde_json::from_value(new_claim.data["run"]["leases"].clone()).unwrap();
    for old in &status.input.fences {
        assert!(new_fences
            .iter()
            .any(|current| current.name == old.name && current.epoch > old.epoch));
    }
    let again = f.output_before(&args, Instant::now() + Duration::from_secs(90));
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("验证已到固定截止"));
    assert_eq!(
        serde_json::from_slice::<Value>(&again.stdout).unwrap(),
        historical
    );
    let after = f.outbox();
    assert_eq!(
        after
            .confirmed()
            .iter()
            .find(|event| event.eid == original_start.eid),
        Some(&original_start)
    );
    assert_eq!(
        after
            .confirmed()
            .iter()
            .find(|event| event.eid == expired.actual.eid),
        Some(&expired.actual)
    );
    assert_eq!(
        after
            .confirmed()
            .iter()
            .filter(|event| event.eid == original_start.eid)
            .count(),
        1
    );
    assert_eq!(
        after
            .confirmed()
            .iter()
            .filter(|event| event.eid == expired.actual.eid)
            .count(),
        1
    );
    assert_eq!(
        after
            .confirmed()
            .iter()
            .find(|event| event.eid == new_claim.eid),
        Some(&new_claim)
    );
    assert!(!after.confirmed().iter().any(|event| matches!(
        event.kind.as_str(),
        "finish.running" | "finish.completed" | "finish.interrupted"
    ) && event.data["run"]["id"]
        == status.input.run_id));
    assert!(!after
        .confirmed()
        .iter()
        .any(|event| event.kind == "task.released" && event.data["run"]["id"] == new_run));
    assert!(!f.runtime.path().join("expired-must-not-execute").exists());
}
