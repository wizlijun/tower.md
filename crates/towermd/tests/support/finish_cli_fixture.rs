//! Shared actual CLI/B0 fixture. Each integration target selects its tests.
use super::{cli_output, support};
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use towermd_core::{
    baseline_review::review_baseline_shared, recovery_contract::baseline::Verdict,
    snapshot_service::PublicationReport,
};

pub(crate) struct Fixture {
    pub(crate) root: tempfile::TempDir,
    pub(crate) runtime: tempfile::TempDir,
}
impl Fixture {
    pub(crate) fn new(commands: &[&str]) -> Self {
        Self::with_modules(commands, false)
    }
    pub(crate) fn with_modules(commands: &[&str], beta: bool) -> Self {
        let mut files=vec![("docs/tower/specs/AC-finish.md","---\ntype: Acceptance\nid: AC-finish\nmodule: A-alpha\ncriteria: [actual CLI finish]\n---\n")];
        if beta {
            files.extend([("beta/b.py","# original beta\n"),("docs/tower/specs/AC-beta.md","---\ntype: Acceptance\nid: AC-beta\nmodule: A-beta\ncriteria: [actual independent module]\n---\n")]);
        }
        let mut f = support::configured_fixture(&files, commands, false);
        let action = support::action(&f, Verdict::Approved);
        let approval = review_baseline_shared(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &action,
            "human:owner",
            "host",
            support::now() + 3,
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
                    || Ok(support::now() + 5)
                )
                .unwrap(),
            PublicationReport::Recorded { .. }
        ));
        support::git(f.root.path(), &["switch", "-qc", "t/T-finish", "main"]);
        let support::Fixture {
            root,
            runtime,
            session,
            outbox,
            ..
        } = f;
        drop(session);
        drop(outbox);
        fs::write(runtime.path().join("host.json"), "\"host\"").unwrap();
        let f = Self { root, runtime };
        let args = [
            "task",
            "add",
            "--id",
            "T-finish",
            "--title",
            "CLI finish",
            "--module",
            "A-alpha",
            "--allow",
            "alpha/**",
            "--acceptance",
            "AC-finish",
        ];
        // Setup has no latency SLA. Only this first Add may start a fresh
        // public request after a proved pre-closure refusal; timed commands
        // below retain their original one-request budgets and assertions.
        for attempt in 0..3 {
            let out = f.command(&args).output().unwrap();
            if out.status.success() {
                let _: Value = cli_output::decode(&out.stdout).unwrap();
                break;
            }
            assert!(
                attempt < 2
                    && out.status.code() == Some(1)
                    && out.stdout.is_empty()
                    && out.stderr == b"towermd: head changed before continuation\n",
                "{args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            // Physical originals only: do not poll status, renew, acknowledge,
            // reconcile, or accept a missing/uncertain source as non-acceptance.
            let catalog = towermd_core::catalog::Catalog::load(f.root.path()).unwrap();
            let directory = f
                .runtime
                .path()
                .join("recovery-outbox")
                .join(catalog.config.project_id());
            let inputs =
                towermd_core::recovery_outbox::RecoveryInputs::read(&directory, &catalog).unwrap();
            // RecoveryInputs intentionally tolerates missing/torn logs for
            // recovery diagnostics. Neither can prove absence for this retry.
            let metadata = fs::symlink_metadata(&directory).unwrap();
            assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
            let mut logs = 0;
            for entry in fs::read_dir(&directory).unwrap() {
                let entry = entry.unwrap();
                if entry.file_name() == ".lock" {
                    continue;
                }
                let metadata = fs::symlink_metadata(entry.path()).unwrap();
                assert!(metadata.is_file() && !metadata.file_type().is_symlink());
                let bytes = fs::read(entry.path()).unwrap();
                assert!(
                    bytes.is_empty() || bytes.ends_with(b"\n"),
                    "torn setup outbox cannot prove non-acceptance"
                );
                logs += 1;
            }
            assert!(
                logs > 0 && (!inputs.confirmed().is_empty() || !inputs.pending().is_empty()),
                "missing/empty setup original logs cannot prove non-acceptance"
            );
            let reread =
                towermd_core::recovery_outbox::RecoveryInputs::read(&directory, &catalog).unwrap();
            assert_eq!(
                reread.digest(),
                inputs.digest(),
                "setup original source changed during inspection"
            );
            let target_task_event = |kind: &str, obj: &str, data: &Value| {
                kind.starts_with("task.")
                    && (obj == "T-finish"
                        || data["task"]["id"] == "T-finish"
                        || data["run"]["task"] == "T-finish")
            };
            assert!(
                !inputs.confirmed().iter().any(|event| {
                    target_task_event(&event.kind, &event.obj, &event.data)
                }) && !inputs.pending().iter().any(|event| {
                    target_task_event(&event.kind, &event.obj, &event.data)
                }),
                "initial Add cannot be retried with any confirmed/pending T-finish original; {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        f.json(&["task", "claim", "T-finish"]);
        f
    }
    pub(crate) fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_towermd"));
        command
            .args(args)
            .current_dir(self.root.path())
            .env("TOWERMD_RUNTIME_DIR", self.runtime.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null());
        command
    }
    #[track_caller]
    pub(crate) fn success(&self, args: &[&str]) -> Output {
        let out = self.command(args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }
    #[track_caller]
    pub(crate) fn json(&self, args: &[&str]) -> Value {
        cli_output::decode(&self.success(args).stdout).unwrap()
    }
    pub(crate) fn snapshot_before_new_finish(&self, target: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        let catalog = towermd_core::catalog::Catalog::load(self.root.path()).unwrap();
        let inputs = self.outbox();
        let claim = inputs
            .confirmed()
            .iter()
            .find(|event| {
                !event.stale && event.kind == "task.claimed" && event.data["task"]["id"] == target
            })
            .expect("fixture must have the actual original Task claim");
        let Some(last) = self.snapshot_requests_before(&catalog, deadline, || {
            self.main_covers_claim(&catalog, claim, deadline)
        }) else {
            return;
        };
        assert!(
            self.main_covers_claim(&catalog, claim, deadline),
            "actual main did not cover the original claim within one 15s/3-request budget; {last}"
        );
    }
    pub(crate) fn snapshot_before_finish_expiry(&self, expiry: &towermd_core::hub_protocol::Event) {
        let deadline = Instant::now() + Duration::from_secs(15);
        let catalog = towermd_core::catalog::Catalog::load(self.root.path()).unwrap();
        let inputs = self.outbox();
        assert_eq!(
            inputs
                .confirmed()
                .iter()
                .filter(|event| event.eid == expiry.eid)
                .collect::<Vec<_>>(),
            vec![expiry],
            "precondition requires the complete actual confirmed expiry"
        );
        assert!(!expiry.stale);
        assert!(!serde_json::to_value(expiry).unwrap()["legacy"]
            .as_bool()
            .unwrap_or(false));
        assert_eq!(expiry.kind, "task.released");
        assert_eq!(expiry.data["finish_expiry"]["schema"], 1);
        assert_eq!(expiry.data["task"]["state"], "ready");
        assert_eq!(expiry.data["run"]["state"], "expired");
        let Some(last) = self.snapshot_requests_before(&catalog, deadline, || {
            self.main_covers_expiry(&catalog, expiry, deadline)
        }) else {
            return;
        };
        assert!(
            self.main_covers_expiry(&catalog, expiry, deadline),
            "actual main did not cover the original expiry within one 15s/3-request budget; {last}"
        );
    }
    // The original bounded request loop is shared; callers retain their own
    // exact actual-event source and immutable-main coverage assertions.
    fn snapshot_requests_before(
        &self,
        catalog: &towermd_core::catalog::Catalog,
        deadline: Instant,
        mut covered: impl FnMut() -> bool,
    ) -> Option<String> {
        let mut last = "no snapshot request issued".to_owned();
        for attempt in 0..3 {
            if covered() {
                return None;
            }
            let output = self.output_before(&["snapshot"], deadline);
            last = format!(
                "attempt {}: status={} stdout={} stderr={}",
                attempt + 1,
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if output.status.success() {
                let report: towermd_core::snapshot_service::SnapshotReport =
                    cli_output::decode(&output.stdout).unwrap_or_else(|error| {
                        panic!("invalid actual SnapshotReport: {error}; {last}")
                    });
                match report.status.as_str() {
                    "written" | "unchanged" | "superseded" => {
                        let commit = report
                            .commit
                            .as_deref()
                            .expect("persisted snapshot report requires its actual commit");
                        let saved = self.snapshot_at(catalog, commit, deadline);
                        assert_eq!(saved.through_seq, report.seq, "{last}");
                        assert_eq!(saved.writer_epoch, report.epoch, "{last}");
                    }
                    // A Busy/older result is only coordination information.
                    // It never claims that the required prefix was persisted.
                    "deferred" => {}
                    _ => panic!("unexpected actual snapshot status; {last}"),
                }
            } else {
                // These exact refusals do not confirm this snapshot attempt.
                // Earlier renewals/maintenance may have appended real events;
                // they never count as persisted coverage of the required event.
                // Unknown or Git-persisted/Hub-confirm failures remain errors.
                assert!(
                    snapshot_prewrite_refusal(output.status.code(), &output.stderr),
                    "{last}"
                );
                // The next loop first rereads actual main coverage. A racing
                // writer that already persisted this exact event ends the
                // precondition without issuing another snapshot request.
            }
        }
        Some(last)
    }
    fn snapshot_at(
        &self,
        catalog: &towermd_core::catalog::Catalog,
        revision: &str,
        deadline: Instant,
    ) -> towermd_core::snapshot::Snapshot {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "snapshot precondition exceeded its original 15s budget"
        );
        let path = format!("{revision}:{}/state/state.json", catalog.config.docs_dir());
        let text =
            towermd_core::repo::git_timeout(self.root.path(), &["show", &path], remaining).unwrap();
        let snapshot: towermd_core::snapshot::Snapshot = serde_json::from_str(&text).unwrap();
        snapshot.validate_metadata().unwrap();
        assert_eq!(snapshot.project_id, catalog.config.project_id());
        snapshot
    }
    fn main_covers_claim(
        &self,
        catalog: &towermd_core::catalog::Catalog,
        claim: &towermd_core::hub_protocol::Event,
        deadline: Instant,
    ) -> bool {
        let Some((_, snapshot)) = self.main_checkpoint_covering(catalog, claim, deadline) else {
            return false;
        };
        let run = claim.data["run"]["id"].as_str().unwrap();
        assert!(
            snapshot
                .projection
                .objects
                .get("Run")
                .is_some_and(|runs| runs.contains_key(run)),
            "actual checkpoint covering the claim must preserve its Run"
        );
        true
    }
    fn main_checkpoint_covering(
        &self,
        catalog: &towermd_core::catalog::Catalog,
        event: &towermd_core::hub_protocol::Event,
        deadline: Instant,
    ) -> Option<(String, towermd_core::snapshot::Snapshot)> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "snapshot precondition exceeded its original 15s budget"
        );
        // This fixture has no remote. Read the real immutable main commit, not
        // a schedule/cache claim or a synthetic Snapshot/receipt.
        let main = towermd_core::repo::git_timeout(
            self.root.path(),
            &["rev-parse", "refs/heads/main"],
            remaining,
        )
        .unwrap();
        let state_path = format!("{}/state/state.json", catalog.config.docs_dir());
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "snapshot precondition exhausted its original budget"
        );
        let present = towermd_core::repo::git_timeout(
            self.root.path(),
            &["ls-tree", "--name-only", &main, "--", &state_path],
            remaining,
        )
        .unwrap();
        if present.is_empty() {
            return None; // Actual valid main has no persisted checkpoint yet.
        }
        assert_eq!(present, state_path, "unexpected actual checkpoint path");
        let snapshot = self.snapshot_at(catalog, &main, deadline);
        if snapshot.through_seq < event.seq {
            return None;
        }
        Some((main, snapshot))
    }
    fn main_covers_expiry(
        &self,
        catalog: &towermd_core::catalog::Catalog,
        expiry: &towermd_core::hub_protocol::Event,
        deadline: Instant,
    ) -> bool {
        let Some((main, snapshot)) = self.main_checkpoint_covering(catalog, expiry, deadline)
        else {
            return false;
        };
        let task = expiry.data["task"]["id"].as_str().unwrap();
        let run = expiry.data["run"]["id"].as_str().unwrap();
        assert_eq!(
            snapshot.projection.objects["Task"][task], expiry.data["task"],
            "actual main must retain the complete core-produced expiry Task"
        );
        assert_eq!(
            snapshot.projection.objects["Run"][run], expiry.data["run"],
            "actual main must retain the complete core-produced expiry Run"
        );
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "snapshot precondition exhausted its original budget"
        );
        let month = expiry.t.get(..7).expect("actual expiry timestamp month");
        let path = format!(
            "{main}:{}/state/events/{month}.jsonl",
            catalog.config.docs_dir()
        );
        let archived =
            towermd_core::repo::git_timeout(self.root.path(), &["show", &path], remaining).unwrap();
        let events = archived
            .lines()
            .map(|line| serde_json::from_str::<towermd_core::hub_protocol::Event>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.eid == expiry.eid)
                .collect::<Vec<_>>(),
            vec![expiry],
            "actual main archive must retain the complete original expiry Event"
        );
        true
    }
    pub(crate) fn output_before(&self, args: &[&str], deadline: Instant) -> Output {
        assert!(
            Instant::now() < deadline,
            "snapshot precondition exhausted its original budget"
        );
        let mut child = self
            .command(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "snapshot precondition exceeded its original 15s budget: stdout={} stderr={}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.wait_with_output().unwrap()
    }
    pub(crate) fn bounded_at(&self, directory: &Path, args: &[&str]) -> Value {
        let mut child = self
            .command(args)
            .current_dir(directory)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "CLI {args:?} exceeded 3s: stdout_bytes={} stderr_bytes={} stdout={} stderr={}",
                    output.stdout.len(),
                    output.stderr.len(),
                    String::from_utf8_lossy(&output.stdout[..output.stdout.len().min(2048)]),
                    String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(2048)])
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        cli_output::decode(&out.stdout).unwrap()
    }
    pub(crate) fn outbox(&self) -> towermd_core::recovery_outbox::RecoveryInputs {
        let catalog = towermd_core::catalog::Catalog::load(self.root.path()).unwrap();
        towermd_core::recovery_outbox::RecoveryInputs::read(
            &self
                .runtime
                .path()
                .join("recovery-outbox")
                .join(catalog.config.project_id()),
            &catalog,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.command(&["daemon", "stop"]).output();
        if std::thread::panicking() {
            eprintln!(
                "{}",
                fs::read_to_string(self.runtime.path().join("daemon.log")).unwrap_or_default()
            );
        }
    }
}
pub(crate) fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(path.exists(), "worker did not reach {}", path.display());
}

// Match only exact preparation/source refusals for bounded snapshot coordination.
// This does not prove that route maintenance/renewals produced no business events.
// An accepted/uncertain or Git-persisted wrapper is never eligible.
pub(crate) fn snapshot_prewrite_refusal(code: Option<i32>, stderr: &[u8]) -> bool {
    code == Some(1)
        && matches!(
            stderr,
            b"towermd: snapshot preparation head changed; prepare again\n"
                | b"towermd: snapshot maintenance prefix changed; prepare again\n"
                | b"towermd: authority changed; reload Catalog\n"
                | b"towermd: authority changed during preparation\n"
                | b"towermd: authority changed during cache probe\n"
        )
}
