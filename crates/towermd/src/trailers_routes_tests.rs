use super::*;

#[test]
fn trailers_wire_accepts_only_directory_task_and_optional_run() {
    let original = json!({"directory":"/actual/project","task":"T","run":"R"});
    assert!(serde_json::from_value::<TrailersRequest>(original.clone()).is_ok());
    let mut implicit = original.clone();
    implicit.as_object_mut().unwrap().remove("run");
    assert!(serde_json::from_value::<TrailersRequest>(implicit).is_ok());
    for (key, value) in [
        ("actor", json!("human:forged")),
        ("proof", json!({"verified":true})),
        ("evidence", json!("E-fake")),
        ("request_id", json!(Uuid::now_v7().to_string())),
        ("candidate", json!("a".repeat(40))),
        ("skip", json!(true)),
        ("guards", json!([])),
    ] {
        let mut forged = original.clone();
        forged[key] = value;
        assert!(
            serde_json::from_value::<TrailersRequest>(forged).is_err(),
            "{key}"
        );
    }
}
#[test]
fn observation_retry_is_independent_of_snapshot_backoff_and_reconstructed_on_registration() {
    let mut schedule = MainObserverSchedule::default();
    assert!(
        schedule.admitted(false, 0),
        "first registration repairs originals even after actual merge"
    );
    schedule.finished(true, 10);
    assert!(!schedule.admitted(false, 9_000));
    assert!(!schedule.admitted(true, 5_009));
    assert!(schedule.admitted(true, 5_010));
    schedule.finished(false, 20);
    assert!(schedule.pending());
    assert!(!schedule.admitted(false, 5_019));
    assert!(schedule.admitted(false, 5_020));
    assert!(MainObserverSchedule::default().admitted(false, 0));
}
#[tokio::test]
async fn trailers_rejects_old_instance_before_project_or_outbox_creation() {
    let (temp, root) = super::super::tests::snapshot_fixture(7200);
    let server = super::super::tests::snapshot_server(
        &root,
        temp.path().join("runtime"),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(1_000_000)),
    );
    let mut headers = HeaderMap::new();
    headers.insert("x-towermd-instance", "old-instance".parse().unwrap());
    let reply = trailers(
        State(server.clone()),
        headers,
        Json(TrailersRequest {
            directory: root,
            task: "T".into(),
            run: None,
        }),
    )
    .await;
    assert_eq!(reply.0, StatusCode::CONFLICT);
    assert!(server.projects.lock().unwrap().is_empty());
    assert!(!server.runtime.join("recovery-outbox").exists());
}

#[tokio::test]
async fn main_only_observation_expires_actual_run_during_snapshot_backoff_but_source_failure_does_not(
) {
    use super::super::tests::{snapshot_fixture, snapshot_git, snapshot_server};
    const START: u64 = 1_800_000_000_000;
    for source_failure in [false, true] {
        let (temp, root) = snapshot_fixture(7_200);
        fs::write(
            root.join("docs/tower/A.md"),
            "---\ntype: Module\nid: A\npaths: ['src/**']\n---\n",
        )
        .unwrap();
        fs::write(root.join("docs/tower/AC.md"), "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [actual independent expiry]\n---\n").unwrap();
        snapshot_git(&root, &["add", "docs"]);
        snapshot_git(&root, &["commit", "-qm", "actual Task expiry source"]);
        snapshot_git(&root, &["switch", "-qc", "t/T"]);
        let unix = Arc::new(AtomicU64::new(START));
        let server = snapshot_server(
            &root,
            temp.path().join("runtime"),
            Arc::new(AtomicU64::new(0)),
            unix.clone(),
        );
        let request = |command| Request {
            directory: root.clone(),
            run: None,
            command,
        };
        server
            .execute(request(TaskCommand::Add {
                task: tasks::TaskSpec {
                    id: "T".into(),
                    title: "actual expiry".into(),
                    module: "A".into(),
                    touches: vec![],
                    allowed_paths: vec!["src/**".into()],
                    depends_on: vec![],
                    acceptance: vec!["AC".into()],
                    decisions: vec![],
                },
            }))
            .unwrap();
        let run = server
            .execute(request(TaskCommand::Claim {
                id: "T".into(),
                harness: None,
                harness_version: None,
            }))
            .unwrap();
        let catalog = Catalog::load(&root).unwrap();
        let project = server.project(&catalog).unwrap();
        let before = project
            .hub
            .lock()
            .unwrap()
            .projection()
            .unwrap()
            .objects
            .clone();
        let original_confirmed = server
            .recovery_inputs(&catalog)
            .unwrap()
            .confirmed()
            .to_vec();
        let expiry = project.hub.lock().unwrap().leases(START)["module:A"].expires_at;
        let main = snapshot_git(&root, &["rev-parse", "main"]);
        // Backoff is only a derived scheduling input, not a claimed Git error
        // or new authority. A wall-clock jump can expire real Hub fences while
        // the existing monotonic snapshot retry deadline remains in the future.
        project.schedule.lock().unwrap().deferred_at(
            "controlled snapshot backoff".into(),
            ClockReading {
                mono_ms: 0,
                unix_ms: START,
            },
            7_200_000,
        );
        let retry = project.schedule.lock().unwrap().next_attempt_at().unwrap();
        assert!(retry > 0);
        *project.main_observer.lock().unwrap() = MainObserverSchedule::default();
        unix.store(expiry + 1, Ordering::SeqCst);
        if source_failure {
            snapshot_git(&root, &["update-ref", "-d", "refs/heads/main"]);
        }
        server.tick().unwrap();
        server.drain_snapshots().await.unwrap();
        assert_eq!(
            project.schedule.lock().unwrap().next_attempt_at(),
            Some(retry),
            "normal observation/expiry never bypasses or resets snapshot backoff"
        );
        let mut expected = before.clone();
        if !source_failure {
            expected.get_mut("Task").unwrap().get_mut("T").unwrap()["state"] = json!("ready");
            expected
                .get_mut("Run")
                .unwrap()
                .get_mut(run["id"].as_str().unwrap())
                .unwrap()["state"] = json!("released");
        }
        assert_eq!(
            project.hub.lock().unwrap().projection().unwrap().objects,
            expected
        );
        // Restore only the exact source ref before read-only physical outbox
        // inspection. This does not run a producer or repair/expire the Run.
        if source_failure {
            assert!(project.main_observer.lock().unwrap().pending());
            snapshot_git(&root, &["update-ref", "refs/heads/main", &main]);
        }
        // Physical confirmation, not the mutable view, proves the original Run
        // was released once. No verification or main merge is synthesized.
        let inputs = server.recovery_inputs(&catalog).unwrap();
        let releases = inputs
            .confirmed()
            .iter()
            .filter(|event| event.kind == "task.released")
            .collect::<Vec<_>>();
        if source_failure {
            assert!(releases.is_empty());
            assert_eq!(inputs.confirmed(), original_confirmed);
        } else {
            assert_eq!(releases.len(), 1);
            assert!(!releases[0].stale);
            assert_eq!(releases[0].actor, "process:towermd-daemon");
            assert_eq!(releases[0].data["task"], expected["Task"]["T"]);
            assert_eq!(
                releases[0].data["run"],
                expected["Run"][run["id"].as_str().unwrap()]
            );
            assert!(inputs
                .confirmed()
                .iter()
                .all(|event| event.kind != "merge.done"
                    && event.kind != "evidence.recorded"
                    && !event.kind.starts_with("finish.")));
        }
        assert_eq!(snapshot_git(&root, &["rev-parse", "main"]), main);
        assert!(snapshot_git(
            &root,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/towermd/snapshot"
            ]
        )
        .is_empty());
        server.shutdown.send_replace(true);
        server.drain_snapshots().await.unwrap();
    }
}
