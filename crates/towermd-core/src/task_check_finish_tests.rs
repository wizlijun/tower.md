//! Real producer observations, followed by actual source/Run changes. No
//! serialized report or hand-built Event is accepted as a check capability.
use super::*;
use crate::{
    ask_fixture::{self as fixture, git, Fixture, NOW},
    hub_protocol::{ReleaseLease, RenewLease},
    project_session::ProjectSession,
    tasks::TaskCommand,
};
use std::{cell::RefCell, fs, path::Path};

fn observe(f: &mut Fixture) -> RecordedCheck {
    recorded_check_shared(&f.session, &f.catalog, "host", None, &mut f.outbox, &|| {
        Ok(NOW + 20)
    })
    .unwrap()
}
fn finish_observation(
    f: &Fixture,
    checked: RecordedCheck,
    now: u64,
) -> Result<
    (
        Task,
        Run,
        crate::project_session::SnapshotSessionId,
        CheckReport,
        u64,
    ),
    GateError,
> {
    checked.finish_observation(&f.session, &f.catalog, "host", &f.outbox, &|| Ok(now))
}
fn assert_no_started(f: &Fixture) {
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|event| event.kind == "finish.started"));
}

#[test]
fn recorded_finish_observation_keeps_actual_report_and_only_its_own_last_check_change() {
    let mut f = fixture::claimed();
    let checked = observe(&mut f);
    let expected = checked.report.clone();
    let run_id = checked.captured.run.id.clone();
    let evidence_id = checked.actual.obj.clone();
    let inputs = f.outbox.read_inputs().unwrap();
    assert!(inputs.confirmed().contains(&checked.actual));
    let (task, run, _, report, _) = finish_observation(&f, checked, NOW + 21).unwrap();
    assert_eq!(report, expected);
    assert!(!report.gates["G3"]);
    assert_eq!(task.state, "running");
    assert_eq!(run.id, run_id);
    assert_eq!(run.extra["last_check"], evidence_id);
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), inputs.digest());
    assert_no_started(&f);
    let request = uuid::Uuid::now_v7().to_string();
    let crate::finish::FinishStart::Queued(ticket) = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &request,
        None,
        &mut f.outbox,
        || Ok(NOW + 22),
    )
    .unwrap() else {
        panic!("a current real observation must retain ordinary admission");
    };
    assert!(!ticket.status().checked_gates["G3"]);
}

#[test]
fn recorded_finish_observation_rejects_real_run_release_and_lost_or_expired_fences() {
    for mutation in ["run_release", "lease_release", "expiry"] {
        let mut f = fixture::claimed();
        let checked = observe(&mut f);
        if mutation == "run_release" {
            ProjectSession::execute_shared(
                &f.session,
                &f.catalog,
                &f.outbox.read_inputs().unwrap(),
                "host",
                TaskCommand::Release,
                None,
                NOW + 21,
                &mut f.outbox,
            )
            .unwrap();
        } else if mutation == "lease_release" {
            f.session
                .lock()
                .unwrap()
                .protocol()
                .unwrap()
                .lease_release(
                    ReleaseLease {
                        host: "host".into(),
                        fences: checked.captured.run.leases.clone(),
                    },
                    NOW + 21,
                )
                .unwrap();
        }
        let now = if mutation == "expiry" {
            NOW + 1_000_000
        } else {
            NOW + 22
        };
        assert!(finish_observation(&f, checked, now).is_err(), "{mutation}");
        assert_no_started(&f);
    }
}

#[test]
fn recorded_finish_observation_accepts_real_renewal_but_rejects_clock_rollback() {
    let mut f = fixture::claimed();
    let checked = observe(&mut f);
    let rollback = observe(&mut f);
    f.session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .lease_renew(
            RenewLease {
                host: "host".into(),
                fences: checked.captured.run.leases.clone(),
                ttl_ms: 300_000,
            },
            NOW + 21,
        )
        .unwrap();
    assert!(finish_observation(&f, checked, NOW + 22).is_ok());
    assert!(finish_observation(&f, rollback, NOW + 19).is_err());
    assert_no_started(&f);
}

// Existing production clock boundary: mutate only after the real diagnostic
// is confirmed, without adding any production hook or accepting a fake report.
fn begin_after_actual_diagnostic(
    f: &mut Fixture,
    mutation: impl FnOnce(),
) -> Result<crate::finish::FinishStart, String> {
    let catalog = f.catalog.clone();
    let directory = f.runtime.path().join("outbox");
    let mutation = RefCell::new(Some(mutation));
    let result = ProjectSession::begin_finish_shared(
        &f.session,
        &f.catalog,
        "host",
        &uuid::Uuid::now_v7().to_string(),
        None,
        &mut f.outbox,
        || {
            let inputs = crate::recovery_outbox::RecoveryInputs::read(&directory, &catalog)?;
            if inputs
                .confirmed()
                .iter()
                .any(crate::task_check_recorded::is_event)
            {
                if let Some(mutation) = mutation.borrow_mut().take() {
                    mutation();
                }
            }
            Ok(NOW + 20)
        },
    );
    assert!(
        mutation.borrow().is_none(),
        "the actual diagnostic boundary must have been reached"
    );
    result
}

fn change_actual_source(root: &Path, authority: &str, mutation: &str) {
    match mutation {
        "same_tree_head" => {
            git(
                root,
                &[
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "same tree different actual HEAD",
                ],
            );
        }
        "main" => {
            let tree = git(root, &["rev-parse", "main^{tree}"]);
            let next = git(
                root,
                &[
                    "commit-tree",
                    &tree,
                    "-p",
                    authority,
                    "-m",
                    "actual main race",
                ],
            );
            git(root, &["update-ref", "refs/heads/main", &next, authority]);
        }
        "config" => {
            let other = tempfile::tempdir().unwrap();
            let worktree = other.path().join("config-main");
            git(
                root,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    worktree.to_str().unwrap(),
                    "main",
                ],
            );
            let path = worktree.join(crate::config::CONFIG_PATH);
            let mut config: serde_yaml::Value =
                serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            config["snapshot_interval"] = serde_yaml::to_value(3600u64).unwrap();
            fs::write(&path, serde_yaml::to_string(&config).unwrap()).unwrap();
            git(&worktree, &["add", crate::config::CONFIG_PATH]);
            git(
                &worktree,
                &["commit", "-qm", "actual authoritative config race"],
            );
            let next = git(&worktree, &["rev-parse", "HEAD"]);
            git(root, &["update-ref", "refs/heads/main", &next, authority]);
        }
        "dirty" => {
            fs::write(root.join("alpha/a.py"), "# dirty actual source\n").unwrap();
        }
        "branch" => {
            git(root, &["switch", "-qc", "t/T-unrelated-observation"]);
        }
        "refs" => {
            git(root, &["tag", "towermd/B999", authority]);
        }
        _ => unreachable!(),
    }
}

#[test]
fn recorded_finish_admission_rejects_actual_head_main_config_dirty_branch_and_ref_changes() {
    for mutation in [
        "same_tree_head",
        "main",
        "config",
        "dirty",
        "branch",
        "refs",
    ] {
        let mut f = fixture::claimed();
        let root = f.root.path().to_owned();
        let authority = f.catalog.revision.clone();
        let result = begin_after_actual_diagnostic(&mut f, || {
            change_actual_source(&root, &authority, mutation)
        });
        assert!(
            result.is_err(),
            "{mutation}: the early tuple cannot bypass the final current check"
        );
        assert_no_started(&f);
    }
}

#[test]
fn recorded_finish_admission_rejects_missing_actual_commit_tree_and_changed_blob_objects() {
    for source in ["commit", "tree", "blob"] {
        let mut f = fixture::claimed();
        fs::write(
            f.root.path().join("alpha/new-readable.py"),
            "# actual allowed object\n",
        )
        .unwrap();
        git(f.root.path(), &["add", "alpha/new-readable.py"]);
        git(
            f.root.path(),
            &["commit", "-qm", "actual changed blob source"],
        );
        let oid = match source {
            "commit" => git(f.root.path(), &["rev-parse", "HEAD"]),
            "tree" => git(f.root.path(), &["rev-parse", "HEAD^{tree}"]),
            "blob" => git(f.root.path(), &["rev-parse", "HEAD:alpha/new-readable.py"]),
            _ => unreachable!(),
        };
        let original = f
            .catalog
            .common_dir
            .join("objects")
            .join(&oid[..2])
            .join(&oid[2..]);
        let withheld = original.with_extension("recorded-check-withheld");
        let result =
            begin_after_actual_diagnostic(&mut f, || fs::rename(&original, &withheld).unwrap());
        fs::rename(&withheld, &original).unwrap();
        assert!(
            result.is_err(),
            "missing {source} cannot reach a new accepted admission"
        );
        assert_no_started(&f);
    }
}
