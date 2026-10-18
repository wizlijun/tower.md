use super::*;
use crate::baseline_review_fixture as fixture;
use fixture::*;

#[test]
fn unapproved_draft_and_checked_out_main_do_not_authorize_publication() {
    let f = fixture();
    let before = git(f.root.path(), &["show-ref"]);
    let history =
        recovery_git::read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    let fence = f.session.lock().unwrap().leases(NOW + 2)["main"].fence();
    let missing = crate::recovery_contract::recorded::request_event_eid(
        &uuid::Uuid::now_v7().to_string(),
        "baseline.approved",
    )
    .unwrap();
    assert!(capture_preparation(&f.catalog, &history, &missing, fence, "host").is_err());
    assert_eq!(before, git(f.root.path(), &["show-ref"]));
    let linked = tempfile::tempdir().unwrap();
    let linked_path = linked.path().join("main's worktree");
    git(
        f.root.path(),
        &["worktree", "add", linked_path.to_str().unwrap(), "main"],
    );
    let reader = Session::for_network(f.root.path()).unwrap();
    let blocked = checked_out_main(&reader, f.root.path())
        .unwrap()
        .expect("checked-out main must be pending, never privately published");
    assert!(blocked.contains(linked_path.to_str().unwrap()));
    assert!(blocked.contains("'\\''"));
    assert!(blocked.contains("switch -c"));
    assert_eq!(before, git(f.root.path(), &["show-ref"]));
    assert!(RepositoryRef::draft("publication-reserved").is_err());
    assert!(!RepositoryRef::publication(&missing)
        .unwrap()
        .name()
        .contains(':'));
}

#[test]
fn fresh_remote_authority_preserves_user_heads_and_rejects_symbolic_tags() {
    let f = fixture();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-q"]);
    git(
        f.root.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(f.root.path(), &["push", "-q", "origin", "main"]);
    let original = Catalog::load(f.root.path()).unwrap();
    let other = tempfile::tempdir().unwrap();
    git(
        other.path(),
        &[
            "clone",
            "-q",
            "-b",
            "main",
            remote.path().to_str().unwrap(),
            ".",
        ],
    );
    std::fs::write(other.path().join("code.txt"), "other authority code\n").unwrap();
    git(other.path(), &["add", "code.txt"]);
    git(
        other.path(),
        &["commit", "-qm", "unrelated code advancement"],
    );
    git(other.path(), &["push", "-q", "origin", "main"]);
    let before_heads = git(
        f.root.path(),
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads/",
        ],
    );
    let before_status = git(f.root.path(), &["status", "--porcelain"]);
    let index = git(
        f.root.path(),
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    let before_index = std::fs::read(&index).unwrap();
    let reader = Session::for_network(f.root.path()).unwrap();
    let authority = fresh_authority(&original, &reader).unwrap();
    assert_eq!(
        authority.catalog.revision,
        git(other.path(), &["rev-parse", "HEAD"])
    );
    assert!(authority.tags.is_empty());
    assert_eq!(
        before_heads,
        git(
            f.root.path(),
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads/"
            ]
        )
    );
    assert_eq!(
        before_status,
        git(f.root.path(), &["status", "--porcelain"])
    );
    assert_eq!(before_index, std::fs::read(&index).unwrap());
    assert!(git(
        f.root.path(),
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/towermd/publication-fetch/"
        ]
    )
    .is_empty());
    git(
        remote.path(),
        &["symbolic-ref", "refs/tags/towermd/B0", "refs/heads/main"],
    );
    assert!(fresh_authority(&authority.catalog, &reader)
        .err()
        .unwrap()
        .contains("symbolic"));
    assert!(git(
        f.root.path(),
        &["for-each-ref", "--format=%(refname)", "refs/tags/towermd/"]
    )
    .is_empty());
}

#[test]
fn original_app_approval_binds_exact_saved_revision_and_never_authorizes_a_later_edit() {
    let mut f = fixture();
    let action = action(&f, Verdict::Approved);
    let reviewed = crate::baseline_review::review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 2,
    )
    .unwrap();
    let eid = reviewed.receipt.result_ref.approval_eid;
    let history =
        recovery_git::read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    let fence = f.session.lock().unwrap().leases(NOW + 3)["main"].fence();
    let intent = capture_preparation(&f.catalog, &history, &eid, fence.clone(), "host").unwrap();
    assert_eq!(intent.approval().layer, "L1");
    assert_eq!(intent.binding(), &action.binding);
    assert_eq!(intent.frozen, f.frozen);
    let before_main = git(f.root.path(), &["rev-parse", "main"]);
    let (edited, request) = next_save(&f);
    f.writer
        .save_draft(&f.session, &f.catalog, &edited, &request, || Ok(NOW + 4))
        .unwrap();
    assert!(
        capture_preparation(&f.catalog, &history, &eid, fence, "host")
            .err()
            .unwrap()
            .contains("binding")
    );
    assert_eq!(before_main, git(f.root.path(), &["rev-parse", "main"]));
    assert!(git(
        f.root.path(),
        &["for-each-ref", "--format=%(refname)", "refs/tags/towermd/"]
    )
    .is_empty());
}

fn approve(f: &mut Fixture) -> String {
    let action = action(f, Verdict::Approved);
    crate::baseline_review::review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &action,
        "human:owner",
        "host",
        NOW + 2,
    )
    .unwrap()
    .receipt
    .result_ref
    .approval_eid
}
fn publish(f: &mut Fixture, eid: &str) -> PublicationReport {
    f.writer
        .publish_baseline(&f.session, &f.catalog, &mut f.outbox, eid, "host", || {
            Ok(NOW + 5)
        })
        .unwrap()
}
fn recorded_commit(report: &PublicationReport) -> &str {
    match report {
        PublicationReport::Recorded { created } => &created.result.commit,
        other => panic!("expected durable publication: {other:?}"),
    }
}
fn pending_commit(report: &PublicationReport) -> &str {
    match report {
        PublicationReport::Pending {
            commit: Some(commit),
            ..
        } => commit,
        other => panic!("expected retained pending commit: {other:?}"),
    }
}
#[test]
fn actual_b0_publication_and_recovery_build_identical_commit_event_once() {
    let mut f = fixture();
    let original = f.catalog.revision.clone();
    let eid = approve(&mut f);
    let before_head = git(f.root.path(), &["rev-parse", "HEAD"]);
    let index = git(
        f.root.path(),
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    let before_index = std::fs::read(&index).unwrap();
    let before_status = git(f.root.path(), &["status", "--porcelain"]);
    let report = publish(&mut f, &eid);
    let commit = recorded_commit(&report).to_owned();
    assert_eq!(git(f.root.path(), &["rev-parse", "main"]), commit);
    assert_eq!(
        git(
            f.root.path(),
            &["rev-parse", "refs/tags/towermd/B0^{commit}"]
        ),
        commit
    );
    assert_eq!(
        git(
            f.root.path(),
            &["rev-list", "--count", &format!("{original}..main")]
        ),
        "1"
    );
    assert_eq!(before_head, git(f.root.path(), &["rev-parse", "HEAD"]));
    assert_eq!(before_index, std::fs::read(&index).unwrap());
    assert_eq!(
        before_status,
        git(f.root.path(), &["status", "--porcelain"])
    );
    assert_eq!(
        git(
            f.root.path(),
            &[
                "rev-parse",
                &format!("refs/towermd/drafts/{}", f.frozen.bundle.id)
            ]
        ),
        f.revision
    );
    assert!(
        !git(f.root.path(), &["ls-tree", "-r", "--name-only", "main"])
            .contains(".towermd/draft-manifest.json")
    );
    let current = Catalog::load(f.root.path()).unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let history = recovery_git::read_recovery_history(&current, &inputs).unwrap();
    let canonical = recovery_git::baseline_created_event(
        &recovery_git::prove_publication(&current, &history, &commit).unwrap(),
    )
    .unwrap();
    assert_eq!(
        canonical.eid,
        crate::events::derived_eid(&commit, "baseline.created").unwrap()
    );
    assert_eq!(
        canonical,
        crate::recovery_outbox::original_of(
            inputs
                .confirmed()
                .iter()
                .find(|e| e.eid == canonical.eid)
                .unwrap()
        )
    );
    assert!(pending_baseline_publications(&current, &inputs)
        .unwrap()
        .is_empty());
    f.catalog = std::sync::Arc::new(current);
    let again = publish(&mut f, &eid);
    assert_eq!(
        serde_json::to_value(report).unwrap(),
        serde_json::to_value(again).unwrap()
    );
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|e| e.kind == "baseline.created")
            .count(),
        1
    );
}
#[test]
fn actual_writer_refuses_unapproved_and_defers_main_in_another_worktree_until_user_frees_it() {
    let mut f = fixture();
    let before = git(f.root.path(), &["show-ref"]);
    let eid = crate::recovery_contract::recorded::request_event_eid(
        &uuid::Uuid::now_v7().to_string(),
        "baseline.approved",
    )
    .unwrap();
    assert!(f
        .writer
        .publish_baseline(&f.session, &f.catalog, &mut f.outbox, &eid, "host", || Ok(
            NOW + 2
        ))
        .is_err());
    assert_eq!(before, git(f.root.path(), &["show-ref"]));
    let eid = approve(&mut f);
    let linked = tempfile::tempdir().unwrap();
    let path = linked.path().join("main's worktree");
    git(
        f.root.path(),
        &["worktree", "add", path.to_str().unwrap(), "main"],
    );
    match publish(&mut f, &eid) {
        PublicationReport::Pending {
            commit: None,
            reason,
            ..
        } => assert!(reason.contains(path.to_str().unwrap())),
        other => panic!("{other:?}"),
    }
    assert!(git(
        f.root.path(),
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/towermd/drafts/publication-*"
        ]
    )
    .is_empty());
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|e| e.kind == "baseline.publication_prepared"));
    // This branch switch is an explicit test of the user's action, not daemon behavior.
    git(&path, &["switch", "-qc", "user-freed-main"]);
    assert!(matches!(
        publish(&mut f, &eid),
        PublicationReport::Recorded { .. }
    ));
}
#[test]
fn each_retained_candidate_main_tag_and_ack_window_resumes_same_commit_after_restart() {
    for point in [
        "BeforePublish",
        "AfterPublish",
        "BeforeTag",
        "AfterTag",
        "BeforeRecorded",
    ] {
        let mut f = fixture();
        let eid = approve(&mut f);
        test_hooks::set(
            f.root.path(),
            point,
            Box::new(|| Err("controlled publication interruption".into())),
        );
        let interrupted = publish(&mut f, &eid);
        let commit = pending_commit(&interrupted).to_owned();
        assert_eq!(
            repository_writer::direct_revision(
                f.root.path(),
                &RepositoryRef::publication(&eid).unwrap()
            )
            .unwrap(),
            Some(commit.clone())
        );
        let original = f.catalog.revision.clone();
        drop(f.writer);
        drop(f.session);
        let current = Catalog::load(f.root.path()).unwrap();
        let session = Mutex::new(
            ProjectSession::open(
                &current.common_dir.join("towermd"),
                &current,
                &f.outbox.read_inputs().unwrap(),
                NOW + 10,
            )
            .unwrap(),
        );
        let writer = SnapshotWriter::default();
        let selectors =
            pending_baseline_publications(&current, &f.outbox.read_inputs().unwrap()).unwrap();
        assert!(selectors.contains(&eid), "{point}: {selectors:?}");
        let resumed = writer
            .publish_baseline(&session, &current, &mut f.outbox, &eid, "host", || {
                Ok(NOW + 11)
            })
            .unwrap();
        assert_eq!(recorded_commit(&resumed), commit, "{point}");
        assert_eq!(
            git(
                f.root.path(),
                &["rev-list", "--count", &format!("{original}..main")]
            ),
            "1",
            "{point}"
        );
        assert_eq!(
            f.outbox
                .read_inputs()
                .unwrap()
                .confirmed()
                .iter()
                .filter(|e| e.kind == "baseline.created")
                .count(),
            1,
            "{point}"
        );
        let latest = Catalog::load(f.root.path()).unwrap();
        assert!(
            pending_baseline_publications(&latest, &f.outbox.read_inputs().unwrap())
                .unwrap()
                .is_empty()
        );
    }
}
fn add_remote(f: &mut Fixture) -> tempfile::TempDir {
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-q"]);
    git(
        f.root.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(f.root.path(), &["push", "-q", "origin", "main"]);
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    remote
}
#[cfg(unix)]
fn rejecting_hook(remote: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = remote.join("hooks/pre-receive");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
#[test]
#[cfg(unix)]
fn remote_tag_rejection_keeps_b0_inactive_and_resumes_original_main_without_republication() {
    let mut f = fixture();
    let remote = add_remote(&mut f);
    let eid = approve(&mut f);
    rejecting_hook(remote.path(), "#!/bin/sh\nwhile read old new name; do\ncase $name in refs/tags/*) exit 1;; esac\ndone\nexit 0\n");
    let pending = publish(&mut f, &eid);
    let commit = pending_commit(&pending).to_owned();
    assert_eq!(git(remote.path(), &["rev-parse", "main"]), commit);
    assert!(git(
        f.root.path(),
        &["for-each-ref", "--format=%(refname)", "refs/tags/towermd/"]
    )
    .is_empty());
    assert!(!f
        .outbox
        .read_inputs()
        .unwrap()
        .confirmed()
        .iter()
        .any(|e| e.kind == "baseline.created"));
    f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
    assert_eq!(pending_commit(&publish(&mut f, &eid)), commit);
    std::fs::remove_file(remote.path().join("hooks/pre-receive")).unwrap();
    let result = publish(&mut f, &eid);
    assert_eq!(recorded_commit(&result), commit);
    assert_eq!(
        git(
            remote.path(),
            &["rev-parse", "refs/tags/towermd/B0^{commit}"]
        ),
        commit
    );
}
#[test]
fn expired_preparation_cannot_publish_but_a_fresh_holder_can_continue_the_same_human_binding() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let mut f = fixture();
    let eid = approve(&mut f);
    let expired = Arc::new(AtomicBool::new(false));
    let fired = expired.clone();
    test_hooks::set(
        f.root.path(),
        "BeforePublish",
        Box::new(move || {
            fired.store(true, Ordering::SeqCst);
            Ok(())
        }),
    );
    let before = git(f.root.path(), &["rev-parse", "main"]);
    let result = f
        .writer
        .publish_baseline(&f.session, &f.catalog, &mut f.outbox, &eid, "host", || {
            Ok(if expired.load(Ordering::SeqCst) {
                NOW + 200_000
            } else {
                NOW + 5
            })
        })
        .unwrap();
    pending_commit(&result);
    assert_eq!(before, git(f.root.path(), &["rev-parse", "main"]));
    assert!(git(
        f.root.path(),
        &["for-each-ref", "--format=%(refname)", "refs/tags/towermd/"]
    )
    .is_empty());
    let result = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &eid,
            "new-holder",
            || Ok(NOW + 200_001),
        )
        .unwrap();
    recorded_commit(&result);
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.kind == "baseline.approved")
            .count(),
        1
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.kind == "baseline.publication_prepared")
            .count(),
        2
    );
}

#[test]
fn hub_loss_preserves_pre_manifest_preparation_and_created_receipts_through_two_rebuilds() {
    for before_manifest in [true, false] {
        let mut f = fixture();
        f.writer
            .write(
                &f.session,
                &f.catalog,
                "host",
                true,
                || Ok(NOW + 2),
                &mut crate::test_support::outbox(&f.catalog),
            )
            .unwrap();
        f.catalog = std::sync::Arc::new(Catalog::load(f.root.path()).unwrap());
        let eid = approve(&mut f);
        if before_manifest {
            test_hooks::set(
                f.root.path(),
                "BeforeCandidate",
                Box::new(|| Err("prepared accepted before manifest crash".into())),
            );
        }
        let initial = publish(&mut f, &eid);
        if before_manifest {
            assert!(matches!(
                initial,
                PublicationReport::Pending {
                    commit: None,
                    stage: PublicationStage::Preparation,
                    ..
                }
            ));
            assert!(git(
                f.root.path(),
                &[
                    "for-each-ref",
                    "--format=%(refname)",
                    "refs/towermd/drafts/publication-*"
                ]
            )
            .is_empty());
        } else {
            recorded_commit(&initial);
        }
        let originals = f
            .outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| {
                matches!(
                    event.kind.as_str(),
                    "baseline.approved" | "baseline.publication_prepared" | "baseline.created"
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        assert!(originals
            .iter()
            .any(|event| event.kind == "baseline.publication_prepared"));
        let visible = Catalog::load(f.root.path()).unwrap();
        let saved = super::super::read_snapshot(&visible).unwrap().unwrap();
        assert!(originals
            .iter()
            .all(|event| event.seq > saved.snapshot.through_seq));
        drop(f.session);
        let directory = f.catalog.common_dir.join("towermd");
        std::fs::remove_dir_all(&directory).unwrap();
        let current = Catalog::load(f.root.path()).unwrap();
        let inputs = f.outbox.read_inputs().unwrap();
        let request = crate::recovery::prepare(
            &current,
            &inputs,
            NOW + 10,
            NOW + 100_000,
            &uuid::Uuid::now_v7().to_string(),
        )
        .unwrap();
        let (_, installed) = f
            .writer
            .confirm_rebuild(
                None,
                &current,
                &request,
                "human:recovery-owner",
                request.expires_at,
                &inputs,
                "host",
                || Ok(NOW + 20),
            )
            .unwrap();
        let current = Catalog::load(f.root.path()).unwrap();
        let resumed = f
            .writer
            .publish_baseline(&installed, &current, &mut f.outbox, &eid, "host", || {
                Ok(NOW + 21)
            })
            .unwrap();
        let publication = recorded_commit(&resumed).to_owned();
        let original_created = f
            .outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|event| {
                event.eid == crate::events::derived_eid(&publication, "baseline.created").unwrap()
            })
            .cloned()
            .expect("first completion has its actual confirmed receipt");
        let current = Catalog::load(f.root.path()).unwrap();
        f.writer
            .write(
                &installed,
                &current,
                "host",
                true,
                || Ok(NOW + 22),
                &mut crate::test_support::outbox(&current),
            )
            .unwrap();
        drop(installed);
        drop(f.outbox);
        std::fs::remove_dir_all(f.runtime.path().join("outbox")).unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
        let current = Catalog::load(f.root.path()).unwrap();
        let mut empty = RecoveryOutbox::open(
            &f.runtime.path().join("outbox"),
            crate::recovery_outbox::RecoveryContext::from_catalog(&current).unwrap(),
        )
        .unwrap();
        let inputs = empty.read_inputs().unwrap();
        let history = recovery_git::read_recovery_history(&current, &inputs).unwrap();
        for original in &originals {
            let restored = match original.kind.as_str() {
                "baseline.approved" => history.baseline_review_original(&original.eid),
                "baseline.created" => history.publication_created_original(&original.eid),
                _ => history
                    .publication_prepared_originals(&current)
                    .unwrap()
                    .into_iter()
                    .find(|event| event.eid == original.eid),
            };
            assert_eq!(
                restored,
                Some(original),
                "preserve the actual original coordinates and full receipt"
            );
        }
        let request = crate::recovery::prepare(
            &current,
            &inputs,
            NOW + 30,
            NOW + 100_000,
            &uuid::Uuid::now_v7().to_string(),
        )
        .unwrap();
        let (_, installed) = f
            .writer
            .confirm_rebuild(
                None,
                &current,
                &request,
                "human:recovery-owner",
                request.expires_at,
                &inputs,
                "host",
                || Ok(NOW + 40),
            )
            .unwrap();
        assert!(
            empty.read_inputs().unwrap().confirmed().is_empty(),
            "internal rebuild must not write original or outer replay confirmations"
        );
        let before_retry = installed
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .contiguous_seq();
        let current = Catalog::load(f.root.path()).unwrap();
        let result = f
            .writer
            .publish_baseline(&installed, &current, &mut empty, &eid, "host", || {
                Ok(NOW + 41)
            })
            .unwrap();
        assert_eq!(recorded_commit(&result), publication);
        assert_eq!(empty.read_inputs().unwrap().confirmed(), &[original_created],
            "explicit original request retry may persist only the exact original accepted receipt, never new coordinates or a replay envelope");
        let after_retry = installed
            .lock()
            .unwrap()
            .protocol()
            .unwrap()
            .subscribe(before_retry, NOW + 41)
            .unwrap();
        assert!(after_retry.events.iter().all(|event| !event.kind.starts_with("baseline.") && event.kind != "request.receipt.restored"),
            "explicit retry must not append a second baseline fact or receipt wrapper to the rebuilt hub");
        let history =
            recovery_git::read_recovery_history(&current, &empty.read_inputs().unwrap()).unwrap();
        let original_created = history
            .publication_created_original(
                &crate::events::derived_eid(&publication, "baseline.created").unwrap(),
            )
            .unwrap();
        assert_eq!(
            original_created.data,
            serde_json::to_value(match &result {
                PublicationReport::Recorded { created } => created,
                _ => unreachable!(),
            })
            .unwrap()
        );
        let saved = super::super::read_snapshot(&current).unwrap().unwrap();
        assert!(saved.archives.values().any(|text| text
            .lines()
            .any(|line| line.contains("request.receipt.restored"))));
        for replacement in [Some(current.revision.as_str()), None] {
            if let Some(other) = replacement {
                git(
                    f.root.path(),
                    &["update-ref", "refs/tags/towermd/B0", other],
                );
            } else {
                git(f.root.path(), &["update-ref", "-d", "refs/tags/towermd/B0"]);
            }
            assert!(
                recovery_git::read_recovery_history(&current, &empty.read_inputs().unwrap())
                    .is_err(),
                "an archived created wrapper cannot replace the actual exact tag proof"
            );
            git(
                f.root.path(),
                &["update-ref", "refs/tags/towermd/B0", &publication],
            );
        }
    }
}

#[test]
fn real_remote_non_fast_forward_reprepares_on_new_code_parent_without_reapproving() {
    let mut f = fixture();
    let remote = add_remote(&mut f);
    let eid = approve(&mut f);
    let other = tempfile::tempdir().unwrap();
    git(
        other.path(),
        &[
            "clone",
            "-q",
            "-b",
            "main",
            remote.path().to_str().unwrap(),
            ".",
        ],
    );
    let clone_path = other.path().to_path_buf();
    test_hooks::set(
        f.root.path(),
        "BeforeMainPush",
        Box::new(move || {
            std::fs::write(clone_path.join("unrelated.py"), "# real competing code\n").unwrap();
            git(&clone_path, &["add", "unrelated.py"]);
            git(&clone_path, &["commit", "-qm", "real competing authority"]);
            git(&clone_path, &["push", "-q", "origin", "main"]);
            Ok(())
        }),
    );
    let result = publish(&mut f, &eid);
    let commit = recorded_commit(&result);
    assert_eq!(git(remote.path(), &["rev-parse", "main"]), commit);
    assert_eq!(
        git(remote.path(), &["show", "main:unrelated.py"]),
        "# real competing code"
    );
    assert_eq!(
        git(f.root.path(), &["rev-parse", &format!("{commit}^")]),
        git(other.path(), &["rev-parse", "HEAD"])
    );
    let inputs = f.outbox.read_inputs().unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "baseline.approved")
            .count(),
        1
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "baseline.publication_prepared")
            .count(),
        2
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "baseline.created")
            .count(),
        1
    );
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .find(|event| event.kind == "baseline.created")
            .unwrap()
            .data["content_hash"],
        f.frozen.content_hash
    );
}
#[test]
#[cfg(unix)]
fn main_push_policy_failure_preserves_candidate_and_all_user_heads_until_same_commit_retry() {
    let mut f = fixture();
    let remote = add_remote(&mut f);
    let eid = approve(&mut f);
    let initial = git(remote.path(), &["rev-parse", "main"]);
    let user_heads = git(
        f.root.path(),
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads/",
        ],
    );
    let index = git(
        f.root.path(),
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    let before_index = std::fs::read(&index).unwrap();
    rejecting_hook(remote.path(), "#!/bin/sh\nexit 1\n");
    let result = publish(&mut f, &eid);
    let commit = pending_commit(&result).to_owned();
    assert_eq!(git(remote.path(), &["rev-parse", "main"]), initial);
    assert_eq!(
        git(
            f.root.path(),
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads/"
            ]
        ),
        user_heads
    );
    assert_eq!(std::fs::read(index).unwrap(), before_index);
    assert!(git(
        f.root.path(),
        &["for-each-ref", "--format=%(refname)", "refs/tags/towermd/"]
    )
    .is_empty());
    assert_eq!(
        f.outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .filter(|event| event.kind == "baseline.publication_prepared")
            .count(),
        1,
        "policy rejection is not a competing-main retry"
    );
    std::fs::remove_file(remote.path().join("hooks/pre-receive")).unwrap();
    assert_eq!(recorded_commit(&publish(&mut f, &eid)), commit);
}
