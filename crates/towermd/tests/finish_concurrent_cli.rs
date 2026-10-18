//! Actual two-worker latency acceptance, isolated from other project fixtures.
#[allow(dead_code)]
#[path = "support/finish_fixture.rs"]
mod support;
// Shared fixture methods are used by different subsets in the two targets.
#[allow(dead_code)]
#[path = "support/finish_cli_fixture.rs"]
mod cli_fixture;
use cli_fixture::{wait_file, Fixture};
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

#[test]
fn finish_two_actual_module_workers_have_independent_reports_and_keep_claim_status_responsive() {
    let f=Fixture::with_modules(&["printf '%s\\n' \"$PWD\" >> \"$TOWERMD_RUNTIME_DIR/finish-workers\"; while [ ! -f \"$TOWERMD_RUNTIME_DIR/finish-release\" ]; do sleep 0.05; done"],true);
    struct Release<'a>(&'a Fixture);
    impl Drop for Release<'_> {
        fn drop(&mut self) {
            let _ = fs::write(self.0.runtime.path().join("finish-release"), "");
        }
    }
    let _release = Release(&f);
    let first = f.json(&["finish"]);
    wait_file(&f.runtime.path().join("finish-workers"));
    f.bounded_at(
        f.root.path(),
        &[
            "task",
            "add",
            "--id",
            "T-beta",
            "--title",
            "parallel beta",
            "--module",
            "A-beta",
            "--allow",
            "beta/**",
            "--acceptance",
            "AC-beta",
        ],
    );
    let beta = f.runtime.path().join("beta-worktree");
    support::git(
        f.root.path(),
        &[
            "worktree",
            "add",
            "-qb",
            "t/T-beta",
            beta.to_str().unwrap(),
            "main",
        ],
    );
    f.bounded_at(&beta, &["task", "claim", "T-beta"]);
    fs::write(beta.join("beta/b.py"), "# committed changed beta\n").unwrap();
    support::git(&beta, &["add", "beta/b.py"]);
    support::git(&beta, &["commit", "-qm", "actual beta Task change"]);
    // Establish the real current-main precondition before timing admission.
    f.snapshot_before_new_finish("T-beta");
    support::git(&beta, &["rebase", "main"]);
    // FR-107 requires asynchronous admission, not a 3s full-proof SLA. Keep
    // both real verifiers held while one existing 15s absolute watchdog covers
    // second admission AND the second worker reaching its execution barrier.
    // Task add/claim/status keep the unmodified 3s bounded_at assertions.
    struct AdmissionChild(Option<std::process::Child>);
    impl Drop for AdmissionChild {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let release = f.runtime.path().join("finish-release");
    assert!(
        !release.exists(),
        "first actual verifier must still be held"
    );
    assert!(matches!(
        first["job_state"].as_str(),
        Some("queued" | "running")
    ));
    assert!(first.get("result").is_none());
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut admission = AdmissionChild(Some(
        f.command(&["finish"])
            .current_dir(&beta)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    while admission.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(
            !release.exists(),
            "admission must return before releasing either verifier"
        );
        if Instant::now() >= deadline {
            let mut child = admission.0.take().unwrap();
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!("second finish admission and worker-ready exceeded one 15s budget while verifier release stayed absent: status={} stdout={} stderr={}", output.status, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = admission.0.take().unwrap().wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "actual second finish failed: status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !release.exists(),
        "a returned admission cannot depend on verifier completion"
    );
    assert!(
        Instant::now() < deadline,
        "second admission exhausted the shared 15s worker budget"
    );
    let second: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_ne!(first["evidence_id"], second["evidence_id"]);
    assert!(matches!(
        second["job_state"].as_str(),
        Some("queued" | "running")
    ));
    assert!(
        second.get("result").is_none(),
        "admission cannot claim verifier success"
    );
    loop {
        assert!(
            !release.exists(),
            "both real workers must reach the barrier before release"
        );
        assert!(Instant::now() < deadline, "second actual module worker did not run concurrently within the same admission/worker 15s budget");
        let workers = fs::read_to_string(f.runtime.path().join("finish-workers")).unwrap();
        if workers.lines().count() == 2 {
            let roots = workers.lines().collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                roots.len(),
                2,
                "two distinct actual verifier worktrees must be waiting"
            );
            assert!(
                roots.iter().all(|root| Path::new(root).is_dir()),
                "both actual verifier worktrees remain present while held"
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    f.bounded_at(f.root.path(), &["status"]);
    assert!(
        !release.exists(),
        "status must respond before either verifier is released"
    );
    fs::write(f.runtime.path().join("finish-release"), "").unwrap();
    let a = f.json(&["finish", "--wait", first["evidence_id"].as_str().unwrap()]);
    let output = f
        .command(&["finish", "--wait", second["evidence_id"].as_str().unwrap()])
        .current_dir(&beta)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let b: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(a["result"], "passed");
    assert_eq!(b["result"], "passed");
    assert_ne!(a["evidence"]["tree"], b["evidence"]["tree"]);
    assert_ne!(
        a["report"]["executions"][0]["stdout_log"],
        b["report"]["executions"][0]["stdout_log"]
    );
    let workers = fs::read_to_string(f.runtime.path().join("finish-workers")).unwrap();
    let roots = workers.lines().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(roots.len(), 2);
    assert_eq!(workers.lines().count(), 2);
}
