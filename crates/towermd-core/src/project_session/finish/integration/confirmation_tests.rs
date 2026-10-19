//! Full confirmed-line fsync failures must remain scheduled durability work.
use super::super::voluntary_fixture as fixture;
use super::*;
use crate::{
    baseline_review::review_baseline_shared,
    finish::{self, FinishStart},
    recovery_contract::baseline::Verdict,
    snapshot_service::PublicationReport,
    tasks::{TaskCommand, TaskSpec},
};
use fixture::git;
use fixture::*;
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

fn passed_fixture() -> (Fixture, finish::FinishOutcome) {
    let mut f = configured_fixture(&[("docs/tower/specs/AC-confirm.md", "---\ntype: Acceptance\nid: AC-confirm\nmodule: A-alpha\ncriteria: [actual complete confirmation]\n---\n")], &["exit 0"], false);
    let action = action(&f, Verdict::Approved);
    let approval = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 3,
    )
    .unwrap();
    assert!(matches!(
        f.writer
            .publish_baseline(
                &f.session,
                &f.catalog,
                &mut f.outbox,
                &approval.receipt.result_ref.approval_eid,
                "host",
                || Ok(NOW + 5)
            )
            .unwrap(),
        PublicationReport::Recorded { .. }
    ));
    git(f.root.path(), &["switch", "-qc", "t/T-confirm", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: "T-confirm".into(),
                title: "actual confirmation repair".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-confirm".into()],
                decisions: vec![],
            },
        },
        None,
        NOW + 10,
        &mut f.outbox,
    )
    .unwrap();
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-confirm".into(),
            harness: None,
            harness_version: None,
        },
        None,
        NOW + 11,
        &mut f.outbox,
    )
    .unwrap();
    fs::write(
        f.root.path().join("alpha/result.py"),
        "# actual sealed result\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "alpha/result.py"]);
    git(
        f.root.path(),
        &["commit", "-qm", "actual verification input"],
    );
    let id = uuid::Uuid::now_v7().to_string();
    let FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &id,
        None,
        &mut f.outbox,
        || Ok(NOW + 30),
    )
    .unwrap() else {
        panic!("new actual queued finish");
    };
    let execution =
        ProjectSession::launch_finish_shared(&f.session, &f.catalog, ticket, &mut f.outbox, || {
            Ok(NOW + 31)
        })
        .unwrap();
    let executed = finish::execute(
        execution,
        Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );
    let done = ProjectSession::record_finish_shared(
        &f.session,
        &f.catalog,
        executed,
        &mut f.outbox,
        || Ok(NOW + 32),
    )
    .unwrap();
    assert_eq!(
        done.evidence.as_ref().unwrap().result,
        finish::EvidenceResult::Passed
    );
    (f, done)
}
fn trailers(f: &mut Fixture, at: u64) -> Result<TaskTrailersOutcome, String> {
    ProjectSession::task_trailers_shared(
        &f.session,
        &f.catalog,
        "host",
        "T-confirm",
        None,
        &mut f.outbox,
        || Ok(at),
    )
}
fn observer(f: &mut Fixture, at: u64) -> Result<MainObservation, String> {
    ProjectSession::observe_task_merges_shared(
        &f.session,
        &f.catalog,
        "host",
        &mut f.outbox,
        || Ok(at),
    )
}
fn cursor(f: &Fixture) -> u64 {
    f.session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .contiguous_seq()
}
fn confirmation_is_complete_but_uncertain(
    f: &Fixture,
    kind: &str,
    expected: impl Fn(&Event) -> bool,
) -> Event {
    let inputs = f.outbox.read_inputs().unwrap();
    let matching = inputs
        .confirmed()
        .iter()
        .filter(|event| event.kind == kind && expected(event))
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "exact original terminal receipt");
    let actual = matching[0].clone();
    assert!(!inputs
        .pending()
        .iter()
        .any(|original| original.eid == actual.eid));
    assert!(!inputs.receipt_confirmed(&actual));
    actual
}
fn assert_repaired_without_hub_append(
    f: &Fixture,
    actual: &Event,
    before: u64,
    digest: &str,
    at: u64,
) {
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.receipt_confirmed(actual));
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.eid == actual.eid)
            .collect::<Vec<_>>(),
        vec![actual]
    );
    assert_eq!(inputs.digest(), digest);
    assert_eq!(cursor(f), before);
    let batch = f
        .session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .subscribe(before, at)
        .unwrap();
    assert!(batch.events.is_empty());
}
#[test]
fn gate_complete_confirmed_line_fsync_failure_is_repaired_before_no_candidate_noop() {
    let (mut f, done) = passed_fixture();
    f.outbox.fail_next_confirmation();
    let error = trailers(&mut f, NOW + 35).unwrap_err();
    assert!(error.contains("uncertain"), "{error}");
    let actual = confirmation_is_complete_but_uncertain(&f, "evidence.recorded", |event| {
        event.data["completed_eid"] == done.actual.eid && event.data["evidence"]["gate"] == "g5"
    });
    assert_eq!(actual.data["evidence"]["gate"], "g5");
    assert_eq!(actual.data["completed_eid"], done.actual.eid);
    assert_eq!(actual.guards, done.status.input.fences);
    let original = recovery_outbox::original_of(&actual);
    let before = cursor(&f);
    let digest = f.outbox.read_inputs().unwrap().digest().to_owned();
    let observed = observer(&mut f, NOW + 36).unwrap();
    assert!(observed.merged.is_empty() && observed.blocked.is_empty());
    assert_repaired_without_hub_append(&f, &actual, before, &digest, NOW + 36);
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        recovery_outbox::original_of(
            inputs
                .confirmed()
                .iter()
                .find(|event| event.eid == actual.eid)
                .unwrap()
        ),
        original
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "finish.completed")
            .collect::<Vec<_>>(),
        vec![&done.actual]
    );
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Task"]["T-confirm"]["state"],
        "verifying"
    );
}
#[test]
fn merge_complete_confirmed_line_fsync_failure_repairs_the_original_after_run_ended() {
    let (mut f, done) = passed_fixture();
    let output = trailers(&mut f, NOW + 35).unwrap();
    git(
        f.root.path(),
        &[
            "commit",
            "--amend",
            "-qm",
            &format!("actual main entry\n\n{}", output.trailers),
        ],
    );
    let commit = git(f.root.path(), &["rev-parse", "HEAD"]);
    git(f.root.path(), &["update-ref", "refs/heads/main", &commit]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    let expiry = f.session.lock().unwrap().leases(NOW + 36)["module:A-alpha"].expires_at;
    f.outbox.fail_next_confirmation();
    let error = observer(&mut f, NOW + 36).unwrap_err();
    assert!(error.contains("uncertain"), "{error}");
    let actual = confirmation_is_complete_but_uncertain(&f, "merge.done", |event| {
        event.data["proof"]["completed"]["eid"] == done.actual.eid
    });
    assert_eq!(
        actual.eid,
        crate::events::derived_eid(&commit, "merge.done").unwrap()
    );
    assert_eq!(actual.data["proof"]["completed"], json!(done.actual));
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Run"][&output.run]["state"],
        "ended"
    );
    // Advance the real system head past the old lease before measuring receipt
    // repair. Its expected expiry/cleanup is separate from any duplicate append.
    let at = expiry + 1;
    ProjectSession::sync_shared(&f.session, &f.catalog, &f.outbox.read_inputs().unwrap(), at)
        .unwrap();
    let before = cursor(&f);
    let digest = f.outbox.read_inputs().unwrap().digest().to_owned();
    let observed = observer(&mut f, at).unwrap();
    assert_eq!(observed.merged, vec![actual.clone()]);
    assert_repaired_without_hub_append(&f, &actual, before, &digest, at);
    assert!(!f
        .session
        .lock()
        .unwrap()
        .leases(at)
        .contains_key("module:A-alpha"));
    assert_eq!(
        f.session.lock().unwrap().projection().unwrap().objects["Task"]["T-confirm"]["state"],
        "merged"
    );
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "task.released"));
}
