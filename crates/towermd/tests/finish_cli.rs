//! Actual binary/daemon finish over a real published B0. No status/report JSON
//! is injected to establish successful verification.
#[allow(dead_code)]
#[path = "support/finish_fixture.rs"]
mod support;
// Shared fixture methods are used by different subsets in the two targets.
#[allow(dead_code)]
#[path = "support/finish_cli_fixture.rs"]
mod cli_fixture;
#[path = "support/cli_output.rs"]
mod cli_output;
use cli_fixture::{snapshot_prewrite_refusal, wait_file, Fixture};
use serde_json::Value;
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn finish_is_async_and_wait_reuses_actual_evidence_after_restart_without_index() {
    let f = Fixture::new(&[
        "printf x >> \"$TOWERMD_RUNTIME_DIR/finish-count\"; sleep 0.2; test -f alpha/a.py",
    ]);
    let queued = f.json(&["finish"]);
    let id = queued["evidence_id"].as_str().unwrap();
    assert!(matches!(
        queued["job_state"].as_str(),
        Some("queued" | "running")
    ));
    assert!(queued.get("result").is_none());
    let done = f.json(&["finish", "--wait", id]);
    assert_eq!(done["evidence_id"], id);
    assert_eq!(done["evidence"]["result"], "passed");
    assert_eq!(done["run_state"], "verifying");
    assert_eq!(done["report"]["commit"], done["evidence"]["commit"]);
    let again = f.json(&["finish", "--wait", id]);
    assert_eq!(again["evidence"], done["evidence"]);
    assert_eq!(
        fs::read(f.runtime.path().join("finish-count")).unwrap(),
        b"x"
    );
    f.success(&["daemon", "stop"]);
    // No job index exists: status registers and reconstructs actual jobs from
    // the real Session before the next explicit wait.
    assert!(!f.runtime.path().join("finish-jobs").exists());
    f.json(&["status"]);
    let restored = f.json(&["finish", "--wait", id]);
    assert_eq!(restored["evidence"], done["evidence"]);
    assert_eq!(restored["actual"], done["actual"]);
    assert_eq!(
        fs::read(f.runtime.path().join("finish-count")).unwrap(),
        b"x"
    );
}
#[test]
fn finish_dirty_input_does_not_execute_and_no_tests_is_nonzero_with_json() {
    let f = Fixture::new(&[]);
    // Coordinate only a real checkpoint covering the actual claim. Deferred
    // responses and a pre-write renewal race are not successful snapshots.
    f.snapshot_before_new_finish("T-finish");
    support::git(f.root.path(), &["rebase", "main"]);
    fs::write(f.root.path().join("alpha/a.py"), "dirty\n").unwrap();
    let before = f.outbox().confirmed().len();
    let dirty = f.command(&["finish"]).output().unwrap();
    assert!(!dirty.status.success());
    assert_eq!(f.outbox().confirmed().len(), before);
    support::git(f.root.path(), &["restore", "alpha/a.py"]);
    support::git(f.root.path(), &["rebase", "main"]);
    let queued = f.json(&["finish"]);
    let id = queued["evidence_id"].as_str().unwrap();
    let done = f.command(&["finish", "--wait", id]).output().unwrap();
    assert!(!done.status.success());
    let value: Value = cli_output::decode(&done.stdout).unwrap();
    assert_eq!(value["job_state"], "completed");
    assert_eq!(value["result"], "no_tests");
    assert!(value["report"]["executions"].as_array().unwrap().is_empty());
}
#[test]
fn finish_report_is_bounded_utf8_hint_and_cannot_assert_passed() {
    let f = Fixture::new(&["exit 7"]);
    let path = f.runtime.path().join("report.txt");
    fs::write(&path, "done; all passed").unwrap();
    let queued = f.json(&["finish", "--report", path.to_str().unwrap()]);
    assert_eq!(queued["report_hint_is_evidence"], false);
    assert!(queued.get("result").is_none());
    let id = queued["evidence_id"].as_str().unwrap();
    let output = f.command(&["finish", "--wait", id]).output().unwrap();
    assert!(!output.status.success());
    let done: Value = cli_output::decode(&output.stdout).unwrap();
    assert_eq!(done["result"], "failed");
    fs::write(&path, vec![b'x'; 1024 * 1024 + 1]).unwrap();
    let invalid = f
        .command(&["finish", "--report", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("1 MiB"));
    fs::write(&path, [0xff]).unwrap();
    let invalid = f
        .command(&["finish", "--report", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("UTF-8"));
}
#[cfg(unix)]
#[test]
fn finish_shutdown_cleans_actual_owned_processes_before_interrupted() {
    let f = Fixture::new(&[
        "sleep 20 & child=$!; printf '%s' \"$child\" > \"$TOWERMD_RUNTIME_DIR/finish-child\"; wait",
    ]);
    let queued = f.json(&["finish"]);
    let id = queued["evidence_id"].as_str().unwrap();
    let pidfile = f.runtime.path().join("finish-child");
    wait_file(&pidfile);
    let pid = fs::read_to_string(&pidfile).unwrap();
    f.success(&["daemon", "stop"]);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let alive = Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        if !alive {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "owned verifier child survived daemon stop"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let result = f.command(&["finish", "--wait", id]).output().unwrap();
    assert!(!result.status.success());
    let done: Value = cli_output::decode(&result.stdout).unwrap();
    assert_eq!(done["job_state"], "interrupted");
    assert!(done.get("evidence").is_none());
}

fn actual_now() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}
#[test]
fn finish_registration_recovers_queued_and_interrupts_orphan_running_without_wait_trigger() {
    use std::sync::Mutex;
    use towermd_core::{
        finish::FinishStart,
        project_session::ProjectSession,
        recovery_outbox::{RecoveryContext, RecoveryOutbox},
    };
    for running in [false, true] {
        let f = Fixture::new(&[
            "printf x >> \"$TOWERMD_RUNTIME_DIR/finish-count\"; test -f alpha/a.py",
        ]);
        f.success(&["daemon", "stop"]);
        // The drained writer may legitimately have advanced main since claim.
        // The simulated old owner must freeze from a genuinely current branch.
        support::git(f.root.path(), &["rebase", "main"]);
        // connect(false) waits for the previous daemon ownership lock; opening
        // the actual Hub afterwards models the previous execution owner.
        let deadline = Instant::now() + Duration::from_secs(15);
        let catalog = towermd_core::catalog::Catalog::load(f.root.path()).unwrap();
        let mut outbox = RecoveryOutbox::open(
            &f.runtime
                .path()
                .join("recovery-outbox")
                .join(catalog.config.project_id()),
            RecoveryContext::from_catalog(&catalog).unwrap(),
        )
        .unwrap();
        let session = loop {
            match ProjectSession::open(
                &catalog.common_dir.join("towermd"),
                &catalog,
                &outbox.read_inputs().unwrap(),
                actual_now(),
            ) {
                Ok(session) => break Mutex::new(session),
                Err(error) => {
                    assert!(
                        error.to_string().contains("lock") || error.to_string().contains("锁"),
                        "{error}"
                    );
                    assert!(
                        Instant::now() < deadline,
                        "daemon did not release actual Hub ownership"
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
            || Ok(actual_now()),
        )
        .unwrap() else {
            panic!()
        };
        let id = ticket.status().input.evidence_id.clone();
        let original_input = ticket.status().input.clone();
        let original_started = outbox
            .read_inputs()
            .unwrap()
            .confirmed()
            .iter()
            .find(|event| event.eid == ticket.status().started_eid)
            .expect("real old owner must confirm its complete original started receipt")
            .clone();
        if running {
            drop(
                ProjectSession::launch_finish_shared(
                    &session,
                    &catalog,
                    ticket,
                    &mut outbox,
                    || Ok(actual_now()),
                )
                .unwrap(),
            );
        } else {
            drop(ticket);
        }
        drop(session);
        drop(outbox);
        f.json(&["status"]); // registration alone, not finish --wait
        let registered = Instant::now();
        // A single 90s watchdog bounds the whole recovery observation, as for
        // finish --wait. FR-107/P2 specify no separate 15s restart SLA.
        let observation_deadline = registered + Duration::from_secs(90);
        let expected = if running {
            "finish.interrupted"
        } else {
            "finish.running"
        };
        let diagnostic = |inputs: &towermd_core::recovery_outbox::RecoveryInputs| {
            let phases = inputs
                .confirmed()
                .iter()
                .filter(|event| {
                    event.kind.starts_with("finish.")
                        && event.data["run"]["finish"]["jobs"].get(&id).is_some()
                })
                .map(|event| (event.kind.as_str(), event.seq, event.stale))
                .collect::<Vec<_>>();
            let pending = inputs
                .pending()
                .iter()
                .map(|event| (event.kind.as_str(), event.eid.as_str()))
                .collect::<Vec<_>>();
            format!(
                "phases={phases:?} pending={pending:?} executor_count={:?}",
                fs::read(f.runtime.path().join("finish-count")).ok()
            )
        };
        let dispatched = loop {
            // Physical outbox reads cannot schedule or resume work. The same
            // absolute watchdog covers dispatch and durable completion.
            let inputs = f.outbox();
            assert!(Instant::now() < observation_deadline,
                "registration did not restore {expected} within recovery90s (dispatch_elapsed_ms={}); {}", registered.elapsed().as_millis(), diagnostic(&inputs));
            let count = fs::read(f.runtime.path().join("finish-count")).ok();
            if running {
                assert!(
                    count.is_none(),
                    "orphan Running must never start an executor; {}",
                    diagnostic(&inputs)
                );
            } else if let Some(count) = &count {
                assert_eq!(
                    count, b"x",
                    "registration must execute once, not retry verification"
                );
            }
            if let Some(event) = inputs.confirmed().iter().find(|event| {
                !event.stale
                    && event.kind == expected
                    && event.data["run"]["finish"]["jobs"].get(&id).is_some()
            }) {
                let job = &event.data["run"]["finish"]["jobs"][&id];
                assert_eq!(
                    job["request_id"], request,
                    "registration must preserve the old request UUID"
                );
                assert_eq!(job["input"], serde_json::to_value(&original_input).unwrap());
                assert_eq!(event.data["run"]["id"], original_input.run_id);
                if running || count.as_deref() == Some(b"x".as_slice()) {
                    break event.clone();
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let dispatch_elapsed = registered.elapsed();
        eprintln!("finish registration evidence_id={id} orphan_running={running} dispatch_elapsed_ms={} recovery_budget_ms=90000 within_original_15s={}", dispatch_elapsed.as_millis(), dispatch_elapsed < Duration::from_secs(15));
        // From here no CLI/HTTP scheduling call is made until the terminal
        // actual confirmed receipt is observed. Completion shares the original
        // absolute watchdog; observing dispatch never extends it.
        let completion_started = Instant::now();
        let terminal = if running {
            dispatched
        } else {
            let completion_deadline = observation_deadline;
            loop {
                let inputs = f.outbox();
                assert!(Instant::now() < completion_deadline,
                    "restored original completion not confirmed within shared recovery90s (dispatch_elapsed_ms={}, completion_elapsed_ms={}); {}",
                    dispatch_elapsed.as_millis(), completion_started.elapsed().as_millis(), diagnostic(&inputs));
                assert_eq!(
                    fs::read(f.runtime.path().join("finish-count")).unwrap(),
                    b"x",
                    "completion observation must not execute again"
                );
                if let Some(event) = inputs.confirmed().iter().find(|event| {
                    !event.stale
                        && event.kind == "finish.completed"
                        && event.data["run"]["finish"]["jobs"].get(&id).is_some()
                }) {
                    assert_eq!(
                        event.data["run"]["finish"]["jobs"][&id]["request_id"],
                        request
                    );
                    assert_eq!(
                        event.data["run"]["finish"]["jobs"][&id]["input"],
                        serde_json::to_value(&original_input).unwrap()
                    );
                    break event.clone();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        eprintln!("finish registration evidence_id={id} orphan_running={running} completion_elapsed_ms={} recovery_budget_ms={} total_observation_elapsed_ms={}",
            completion_started.elapsed().as_millis(), if running { 0 } else { 90000 }, registered.elapsed().as_millis());
        let before_wait = f.outbox();
        assert_eq!(
            before_wait
                .confirmed()
                .iter()
                .find(|event| event.eid == original_started.eid),
            Some(&original_started)
        );
        assert_eq!(
            before_wait
                .confirmed()
                .iter()
                .filter(|event| event.eid == terminal.eid)
                .collect::<Vec<_>>(),
            vec![&terminal]
        );
        let result = f.command(&["finish", "--wait", &id]).output().unwrap();
        let restored: Value = cli_output::decode(&result.stdout).unwrap();
        assert_eq!(restored["evidence_id"], id);
        assert_eq!(
            restored["input"],
            serde_json::to_value(&original_input).unwrap()
        );
        assert_eq!(
            restored["actual"],
            serde_json::to_value(&terminal).unwrap(),
            "wait reads the previously observed exact actual terminal receipt"
        );
        let after_wait = f.outbox();
        assert_eq!(
            after_wait
                .confirmed()
                .iter()
                .find(|event| event.eid == original_started.eid),
            Some(&original_started)
        );
        assert_eq!(
            after_wait
                .confirmed()
                .iter()
                .filter(|event| event.eid == terminal.eid)
                .collect::<Vec<_>>(),
            vec![&terminal]
        );
        for event in after_wait
            .confirmed()
            .iter()
            .filter(|event| event.kind.starts_with("finish."))
        {
            let job = &event.data["run"]["finish"]["jobs"][&id];
            assert_eq!(
                job["request_id"], request,
                "registration/wait must not allocate another request"
            );
            assert_eq!(job["input"], serde_json::to_value(&original_input).unwrap());
        }
        if running {
            assert!(!result.status.success());
            assert_eq!(restored["job_state"], "interrupted");
            assert!(!f.runtime.path().join("finish-count").exists());
        } else {
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(restored["result"], "passed");
            assert_eq!(restored["report"], terminal.data["report"]);
            assert_eq!(restored["evidence"], terminal.data["evidence"]);
            assert_eq!(
                fs::read(f.runtime.path().join("finish-count")).unwrap(),
                b"x"
            );
        }
    }
}
#[test]
fn finish_same_active_frozen_input_returns_original_id_without_second_executor() {
    let f=Fixture::new(&["printf x >> \"$TOWERMD_RUNTIME_DIR/finish-count\"; while [ ! -f \"$TOWERMD_RUNTIME_DIR/finish-release\" ]; do sleep 0.05; done"]);
    struct Release<'a>(&'a Fixture);
    impl Drop for Release<'_> {
        fn drop(&mut self) {
            let _ = fs::write(self.0.runtime.path().join("finish-release"), "");
        }
    }
    let _release = Release(&f);
    let first = f.json(&["finish"]);
    let id = first["evidence_id"].as_str().unwrap();
    wait_file(&f.runtime.path().join("finish-count"));
    f.json(&["status"]);
    let repeated = f.json(&["finish"]);
    assert_eq!(repeated["evidence_id"], id);
    assert_eq!(repeated["job_state"], "running");
    assert!(repeated.get("result").is_none());
    fs::write(f.runtime.path().join("finish-release"), "").unwrap();
    let completed = f.json(&["finish", "--wait", id]);
    assert_eq!(completed["result"], "passed");
    assert_eq!(
        fs::read(f.runtime.path().join("finish-count")).unwrap(),
        b"x"
    );
}

#[test]
fn finish_wait_rejects_unknown_foreign_host_branch_and_explicit_run_without_new_intents() {
    let f = Fixture::new(&["exit 0"]);
    let queued = f.json(&["finish"]);
    let id = queued["evidence_id"].as_str().unwrap();
    let done = f.json(&["finish", "--wait", id]);
    let before = f.outbox();
    let unknown = format!("E-{}", uuid::Uuid::now_v7());
    assert!(!f
        .command(&["finish", "--wait", &unknown])
        .output()
        .unwrap()
        .status
        .success());
    assert!(!f
        .command(&["--run", "R-not-the-original", "finish", "--wait", id])
        .output()
        .unwrap()
        .status
        .success());
    support::git(f.root.path(), &["switch", "-qc", "viewer"]);
    let branch = f.command(&["finish", "--wait", id]).output().unwrap();
    assert!(!branch.status.success());
    assert!(String::from_utf8_lossy(&branch.stderr).contains("worktree/branch"));
    support::git(f.root.path(), &["switch", "-q", "t/T-finish"]);
    let foreign = f.runtime.path().join("foreign-clone");
    support::git(
        f.root.path(),
        &[
            "clone",
            "--no-local",
            "-q",
            f.root.path().to_str().unwrap(),
            foreign.to_str().unwrap(),
        ],
    );
    let clone = f
        .command(&["finish", "--wait", id])
        .current_dir(&foreign)
        .output()
        .unwrap();
    assert!(!clone.status.success());
    assert!(
        String::from_utf8_lossy(&clone.stderr).contains("clone"),
        "{}",
        String::from_utf8_lossy(&clone.stderr)
    );
    assert_eq!(f.outbox().digest(), before.digest());
    f.success(&["daemon", "stop"]);
    fs::write(f.runtime.path().join("host.json"), "\"different-host\"").unwrap();
    let host = f.command(&["finish", "--wait", id]).output().unwrap();
    assert!(!host.status.success());
    assert!(String::from_utf8_lossy(&host.stderr).contains("identity conflict"));
    assert_eq!(f.outbox().digest(), before.digest());
    f.success(&["daemon", "stop"]);
    fs::write(f.runtime.path().join("host.json"), "\"host\"").unwrap();
    let same = f.json(&["finish", "--wait", id]);
    assert_eq!(same["evidence"], done["evidence"]);
    assert_eq!(same["actual"], done["actual"]);
}
#[test]
fn finish_http_has_no_actor_commands_fences_or_skip_bypass() {
    let f = Fixture::new(&["exit 0"]);
    let endpoint: Value =
        serde_json::from_slice(&fs::read(f.runtime.path().join("endpoint.json")).unwrap()).unwrap();
    let port = endpoint["port"].as_u64().unwrap();
    let instance = endpoint["instance"].as_str().unwrap();
    let before = f.outbox();
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    for field in ["actor", "commands", "fences", "skip"] {
        let mut request =
            serde_json::json!({"directory":f.root.path(),"run":null,"wait":null,"report":null});
        request[field] = serde_json::json!("untrusted override");
        let response = client
            .post(format!("http://127.0.0.1:{port}/finish"))
            .header("x-towermd-instance", instance)
            .json(&request)
            .send()
            .unwrap();
        assert_eq!(
            response.status(),
            reqwest::StatusCode::UNPROCESSABLE_ENTITY,
            "{field}"
        );
    }
    assert_eq!(f.outbox().digest(), before.digest());
    assert!(!f
        .outbox()
        .pending()
        .iter()
        .any(|event| event.kind.starts_with("finish.")));
}

#[test]
fn snapshot_finish_precondition_retries_only_exact_plain_prewrite_guards() {
    for plain in [
        b"towermd: snapshot preparation head changed; prepare again\n".as_slice(),
        b"towermd: authority changed; reload Catalog\n".as_slice(),
        b"towermd: authority changed during preparation\n".as_slice(),
        b"towermd: authority changed during cache probe\n".as_slice(),
    ] {
        assert!(snapshot_prewrite_refusal(Some(1), plain));
        for code in [None, Some(0), Some(2)] {
            assert!(!snapshot_prewrite_refusal(code, plain));
        }
        for extra in [b"\n".as_slice(), b"extra line\n".as_slice()] {
            assert!(!snapshot_prewrite_refusal(
                Some(1),
                &[plain, extra].concat()
            ));
        }
        for prefix in [
            "towermd: Git 快照 abc 已写入，但 hub 确认失败；下次从 main/private 补确认：",
            "towermd: uncertain: ",
            "towermd: accepted request requires synchronization: ",
        ] {
            let wrapped = format!(
                "{prefix}{}",
                std::str::from_utf8(plain)
                    .unwrap()
                    .trim_start_matches("towermd: ")
            );
            assert!(!snapshot_prewrite_refusal(Some(1), wrapped.as_bytes()));
        }
    }
    for unknown in [
        "towermd: authority changed\n",
        "towermd: SourceChanged: source changed before recovery Git boundary\n",
        "towermd: private snapshot source changed during request\n",
    ] {
        assert!(!snapshot_prewrite_refusal(Some(1), unknown.as_bytes()));
    }
}
