use crate::hub::{Hub, NewEvent};
use crate::snapshot::archive_events;
use crate::snapshot_git::{WriteOutcome, WriteRequest};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Mutex;

use crate::catalog::Catalog;
use crate::{repo, snapshot_git, snapshot_service};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecoveryRequest {
    pub status: String,
    pub request_id: String,
    pub project_id: String,
    pub main_revision: String,
    pub source_commit: String,
    pub source_seq: u64,
    pub seq_boundary: u64,
    pub epoch_boundary: u64,
}

pub struct AppConfirmation {
    request_id: String,
    actor: String,
    expires_at: u64,
}

pub(crate) fn execute(
    hub: &Mutex<Hub>,
    catalog: &Catalog,
    request: &RecoveryRequest,
    confirmation: &AppConfirmation,
    host: &str,
    mut now: impl FnMut() -> Result<u64, String>,
) -> Result<snapshot_service::SnapshotReport, String> {
    validate_confirmation(request, confirmation, now()?)?;
    let watermark = {
        let hub = hub.lock().map_err(|_| "hub 锁损坏")?;
        if hub.last_seq() != 0 {
            return Err("hub journal 仍有可读历史，拒绝将其作为丢失数据覆盖".into());
        }
        (hub.last_seq(), hub.epoch_high_water())
    };
    crate::origin::refresh_origin_main(&catalog.root)?;
    let current = Catalog::load(&catalog.root)?;
    if prepare(&current)? != *request {
        return Err("main 或恢复边界已变化，须重新请求 App 确认".into());
    }
    let saved = snapshot_service::read_snapshot(&current)?.ok_or("恢复快照消失")?;
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let mut staged = Hub::stage_recovery(
        temporary.path(),
        &saved.snapshot,
        request.seq_boundary,
        request.epoch_boundary,
    )?;
    let time = now()?;
    let mut written = recovery_event(
        host,
        "snapshot.written",
        json!({"commit":saved.commit,"seq":saved.snapshot.through_seq,"epoch":saved.snapshot.writer_epoch}),
    );
    written.eid = crate::hub::derived_eid(&saved.commit, "snapshot.written")?;
    staged.append_receipt(written, None, time)?;
    let mut approval = recovery_event(
        host,
        "hub.rebuild.approved",
        json!({"request_id":request.request_id,"source_commit":request.source_commit}),
    );
    approval.actor = confirmation.actor.clone();
    staged.append(approval, None, time)?;
    staged.append(recovery_event(host, "hub.rebuilt", json!({"source_commit":request.source_commit,"seq_boundary":request.seq_boundary,"epoch_boundary":request.epoch_boundary})), None, time)?;
    if let Some(runs) = saved.snapshot.projection.objects.get("Run") {
        for run in runs.values().filter(|run| run["state"] == "running") {
            let task_id = run["task"].as_str().ok_or("恢复 Run 缺少 task")?;
            let mut task = saved
                .snapshot
                .projection
                .objects
                .get("Task")
                .and_then(|tasks| tasks.get(task_id))
                .cloned()
                .ok_or("恢复 Run 引用未知 Task")?;
            let mut run = run.clone();
            task["state"] = "ready".into();
            run["state"] = "released".into();
            let mut event = recovery_event(host, "task.state", json!({"task":task,"run":run}));
            event.layer = "L4".into();
            event.obj = task_id.into();
            staged.append(event, None, time)?;
        }
    }
    let fence = snapshot_service::acquire_main(&mut staged, host, time)?;
    let checkpoint = staged.checkpoint(&request.project_id, &fence, time)?;
    let mut archives = saved.archives;
    for event in archive_events(
        &staged.events(0),
        request.seq_boundary,
        checkpoint.through_seq,
    )? {
        let month = chrono::DateTime::parse_from_rfc3339(&event.t)
            .map_err(|error| error.to_string())?
            .format("%Y-%m")
            .to_string();
        let content = archives.entry(month).or_default();
        content.push_str(&serde_json::to_string(&event).map_err(|error| error.to_string())?);
        content.push('\n');
    }
    let write = WriteRequest {
        base: current.revision.clone(),
        docs_dir: current.config.docs_dir().into(),
        state_json: format!(
            "{}\n",
            serde_json::to_string_pretty(&checkpoint).map_err(|error| error.to_string())?
        ),
        archives,
        through_seq: checkpoint.through_seq,
        writer_epoch: fence.epoch,
    };
    let outcome = snapshot_git::write_snapshot(&current.root, &write, || {
        let time = now()?;
        validate_confirmation(request, confirmation, time)?;
        snapshot_service::renew_fence(&mut staged, host, &fence, time)
    })?;
    let commit = match outcome {
        WriteOutcome::Written { commit } => commit,
        other => return Err(format!("恢复边界未写入，不激活服务：{other:?}")),
    };
    (|| {
        snapshot_service::record_written(&mut staged, host, &commit, &checkpoint, now()?)?;
        let mut hub = hub.lock().map_err(|_| "hub 锁损坏")?;
        if hub.last_seq() != 0 || (hub.last_seq(), hub.epoch_high_water()) != watermark {
            return Err("hub 在恢复期间已变化，拒绝覆盖；未激活恢复 seed".into());
        }
        hub.install_recovery(&staged)
    })()
        .map_err(|error| format!("恢复边界 {commit} 已写 main，本地安装失败；须重新读取 main 的更高边界，不可重用旧请求：{error}"))?;
    Ok(snapshot_service::SnapshotReport {
        status: "rebuilt".into(),
        commit: Some(commit),
        seq: checkpoint.through_seq,
        epoch: fence.epoch,
        reason: None,
    })
}

fn validate_confirmation(
    request: &RecoveryRequest,
    confirmation: &AppConfirmation,
    now: u64,
) -> Result<(), String> {
    if confirmation.request_id != request.request_id
        || confirmation.expires_at <= now
        || !confirmation.actor.strip_prefix("human:").is_some_and(|id| {
            !id.is_empty() && !id.contains([':', '/']) && !id.chars().any(char::is_whitespace)
        })
    {
        return Err("恢复需要匹配且未过期的 App 人工确认".into());
    }
    Ok(())
}

fn recovery_event(host: &str, kind: &str, data: serde_json::Value) -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: host.into(),
        actor: "process:towermd-daemon".into(),
        kind: kind.into(),
        layer: "L3".into(),
        obj: "main".into(),
        data,
        fence: None,
    }
}

pub fn prepare(catalog: &Catalog) -> Result<RecoveryRequest, String> {
    let saved = snapshot_service::read_snapshot(catalog)?.ok_or("main 没有可恢复的快照")?;
    let tail = repo::git(
        &catalog.root,
        &[
            "rev-list",
            &format!("{}..{}", saved.commit, catalog.revision),
        ],
    )?;
    for commit in tail.lines() {
        let message = repo::git(
            &catalog.root,
            &["show", "--no-patch", "--format=%B", commit],
        )?;
        if message
            .trim_end()
            .rsplit("\n\n")
            .next()
            .is_some_and(|footer| {
                footer
                    .lines()
                    .any(|line| line.trim_start().starts_with("Towermd-"))
            })
        {
            return Err(
                "快照之后有当前版本尚不支持恢复的 Towermd trailer；请升级恢复器，不可忽略这些提交"
                    .into(),
            );
        }
    }
    let history = repo::git(
        &catalog.root,
        &[
            "log",
            "--format=%H",
            &catalog.revision,
            "--",
            &format!(":(literal){}/state/state.json", catalog.config.docs_dir()),
        ],
    )?;
    for commit in history.lines() {
        if let Some(trailer) = snapshot_git::read_trailer(&catalog.root, commit)? {
            if (trailer.epoch, trailer.seq)
                > (saved.snapshot.writer_epoch, saved.snapshot.through_seq)
            {
                return Err("main 曾有更高恢复水位；拒绝从回退快照恢复".into());
            }
        }
    }
    let mut request = RecoveryRequest {
        status: "awaiting_app_confirmation".into(),
        request_id: String::new(),
        project_id: catalog.config.project_id().into(),
        main_revision: catalog.revision.clone(),
        source_commit: saved.commit,
        source_seq: saved.snapshot.through_seq,
        seq_boundary: saved
            .snapshot
            .through_seq
            .checked_add(1 << 32)
            .ok_or("恢复 seq 边界溢出")?,
        epoch_boundary: saved
            .snapshot
            .epoch_high_water
            .checked_add(1 << 32)
            .ok_or("恢复 epoch 边界溢出")?,
    };
    request.request_id = hex::encode(Sha256::digest(
        serde_json::to_vec(&request).map_err(|error| error.to_string())?,
    ));
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::Hub;
    use crate::init::{init, InitOptions};
    use crate::snapshot_service::SnapshotWriter;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end_matches('\n')
            .into()
    }

    fn fixture() -> (tempfile::TempDir, Catalog, Mutex<Hub>) {
        let temporary = tempfile::tempdir().unwrap();
        git(temporary.path(), &["init", "-qb", "main"]);
        init(temporary.path(), &InitOptions { docs_dir: None }).unwrap();
        git(temporary.path(), &["add", "."]);
        git(temporary.path(), &["commit", "-qm", "fixture"]);
        git(temporary.path(), &["switch", "-qc", "recovery-fixture"]);
        let catalog = Catalog::load(temporary.path()).unwrap();
        let hub = Mutex::new(Hub::open(&catalog.common_dir.join("towermd")).unwrap());
        let report = SnapshotWriter::default()
            .write(&hub, &catalog, "host", true, || Ok(1000))
            .unwrap();
        assert_eq!(report.status, "written");
        let catalog = Catalog::load(temporary.path()).unwrap();
        (temporary, catalog, hub)
    }

    #[test]
    fn request_only_requires_app_confirmation_and_changes_no_state() {
        let (temporary, catalog, hub) = fixture();
        let journal = fs::read(catalog.common_dir.join("towermd/journal.json")).unwrap();
        let request = prepare(&catalog).unwrap();
        assert_eq!(request.status, "awaiting_app_confirmation");
        assert_eq!(request.project_id, catalog.config.project_id());
        assert_eq!(request.seq_boundary, request.source_seq + (1 << 32));
        assert!(request.epoch_boundary > 1 << 32);
        assert_eq!(request, prepare(&catalog).unwrap());
        assert_eq!(
            git(temporary.path(), &["rev-parse", "main"]),
            catalog.revision
        );
        assert_eq!(
            fs::read(catalog.common_dir.join("towermd/journal.json")).unwrap(),
            journal
        );
        assert!(hub.lock().unwrap().last_seq() < request.seq_boundary);
    }

    #[test]
    fn future_trailer_after_snapshot_is_not_silently_discarded() {
        let (temporary, _catalog, _hub) = fixture();
        git(temporary.path(), &["switch", "main"]);
        git(
            temporary.path(),
            &[
                "commit",
                "--allow-empty",
                "-qm",
                "future action\n\nTowermd-Task: T-future\nTowermd-Run: R-future",
            ],
        );
        let catalog = Catalog::load(temporary.path()).unwrap();
        assert!(prepare(&catalog).unwrap_err().contains("trailer"));
    }

    fn confirmation(request: &RecoveryRequest) -> AppConfirmation {
        AppConfirmation {
            request_id: request.request_id.clone(),
            actor: "human:test".into(),
            expires_at: 10_000,
        }
    }

    #[cfg(unix)]
    fn rebuild_fetch_probe(test_name: &str, concurrent_write: bool) {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};

        let Ok(gates) = std::env::var("TOWERMD_RECOVERY_FETCH_GATES") else {
            let wrapper = tempfile::tempdir().unwrap();
            let original_git = Command::new("which").arg("git").output().unwrap();
            assert!(original_git.status.success());
            let original_git = String::from_utf8(original_git.stdout).unwrap();
            let program = wrapper.path().join("git");
            fs::write(
                &program,
                r#"#!/bin/sh
for argument in "$@"; do
    if test "$argument" = fetch; then
        gate="$TOWERMD_RECOVERY_FETCH_GATES/1"
        if test -f "$gate.entered"; then gate="$TOWERMD_RECOVERY_FETCH_GATES/2"; fi
        touch "$gate.entered"
        attempts=0
        while ! test -f "$gate.release"; do
            attempts=$((attempts + 1))
            if test "$attempts" -ge 200; then exit 99; fi
            sleep 0.05
        done
        break
    fi
done
exec "$TOWERMD_RECOVERY_REAL_GIT" "$@"
"#,
            )
            .unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    &format!("recovery::tests::{test_name}"),
                    "--nocapture",
                ])
                .env("TOWERMD_RECOVERY_FETCH_GATES", wrapper.path())
                .env("TOWERMD_RECOVERY_REAL_GIT", original_git.trim())
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        wrapper.path().display(),
                        std::env::var("PATH").unwrap()
                    ),
                )
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        };
        let (temporary, catalog, _original) = fixture();
        let request = prepare(&catalog).unwrap();
        let remote = tempfile::tempdir().unwrap();
        git(
            temporary.path(),
            &["clone", "--bare", ".", remote.path().to_str().unwrap()],
        );
        git(
            temporary.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        let target_dir = temporary.path().join("lost-hub");
        let hub = Mutex::new(Hub::open(&target_dir).unwrap());
        let writer = SnapshotWriter::default();
        let approval = confirmation(&request);
        let mut available = Vec::new();
        let mut writing_busy = Vec::new();
        let mut journal = None;
        let event = recovery_event("other-host", "other-project.tick", json!({"kept":true}));
        let result = std::thread::scope(|scope| {
            let worker = scope
                .spawn(|| writer.rebuild(&hub, &catalog, &request, &approval, "host", || Ok(2000)));
            for phase in 1..=2 {
                let gate = Path::new(&gates).join(phase.to_string());
                let started = Instant::now();
                while !gate.with_extension("entered").exists() {
                    assert!(
                        started.elapsed() < Duration::from_secs(5),
                        "fetch {phase} never started"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                match hub.try_lock() {
                    Ok(mut target) => {
                        available.push(true);
                        if concurrent_write && phase == 2 {
                            target.append(event.clone(), None, 2000).unwrap();
                            journal = Some(fs::read(target_dir.join("journal.json")).unwrap());
                        }
                    }
                    Err(_) => available.push(false),
                }
                writing_busy.push(
                    writer
                        .rebuild(&hub, &catalog, &request, &approval, "host", || Ok(2000))
                        .is_err()
                        && writer
                            .write(&hub, &catalog, "host", true, || Ok(2000))
                            .unwrap()
                            .status
                            == "deferred",
                );
                fs::write(gate.with_extension("release"), "").unwrap();
            }
            worker.join().unwrap()
        });
        assert_eq!(
            available,
            [true, true],
            "Catalog/write_snapshot fetch held hub mutex"
        );
        assert_eq!(
            writing_busy,
            [true, true],
            "single-writer lock must span both fetches"
        );
        crate::origin::refresh_origin_main(temporary.path()).unwrap();
        let current = Catalog::load(temporary.path()).unwrap();
        if concurrent_write {
            let error = result.unwrap_err();
            assert!(
                error.contains(&current.revision)
                    && error.contains("已写 main")
                    && error.contains("变化"),
                "{error}"
            );
            assert_ne!(current.revision, request.main_revision);
            let next = prepare(&current).unwrap();
            assert!(next.seq_boundary > request.seq_boundary);
            assert!(next.epoch_boundary > request.epoch_boundary);
            assert_eq!(
                fs::read(target_dir.join("journal.json")).unwrap(),
                journal.unwrap()
            );
            let target = hub.lock().unwrap();
            assert_eq!(target.last_seq(), 1);
            assert_eq!(target.events(0)[0].eid, event.eid);
            assert!(target.base_snapshot().is_none());
            assert!(target.snapshot_ack().is_none());
            assert!(snapshot_service::validate_authority(&target, &current).is_err());
            drop(target);
            drop(hub);
            let reopened = Hub::open(&target_dir).unwrap();
            assert_eq!(reopened.events(0)[0].eid, event.eid);
            assert!(reopened.base_snapshot().is_none());
        } else {
            let report = result.unwrap();
            assert_eq!(report.status, "rebuilt");
            assert_eq!(report.commit.as_deref(), Some(current.revision.as_str()));
            snapshot_service::validate_authority(&hub.lock().unwrap(), &current).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn rebuild_fetch_releases_hub_but_keeps_single_writer() {
        rebuild_fetch_probe("rebuild_fetch_releases_hub_but_keeps_single_writer", false);
    }

    #[cfg(unix)]
    #[test]
    fn rebuild_fetch_concurrent_write_preserves_target_without_seed() {
        rebuild_fetch_probe(
            "rebuild_fetch_concurrent_write_preserves_target_without_seed",
            true,
        );
    }

    #[test]
    fn rebuild_rejects_nonempty_or_poisoned_target_before_fetch() {
        let (temporary, catalog, original) = fixture();
        let request = prepare(&catalog).unwrap();
        git(
            temporary.path(),
            &["remote", "add", "origin", "missing-remote"],
        );
        let writer = SnapshotWriter::default();
        let error = writer
            .rebuild(
                &original,
                &catalog,
                &request,
                &confirmation(&request),
                "host",
                || Ok(2000),
            )
            .unwrap_err();
        assert!(error.contains("仍有可读历史"), "{error}");
        let target = Mutex::new(Hub::open(&temporary.path().join("lost-hub")).unwrap());
        assert!(std::panic::catch_unwind(|| {
            let _guard = target.lock().unwrap();
            panic!("poison target mutex");
        })
        .is_err());
        let error = writer
            .rebuild(
                &target,
                &catalog,
                &request,
                &confirmation(&request),
                "host",
                || Ok(2000),
            )
            .unwrap_err();
        assert!(error.contains("hub 锁损坏"), "{error}");
        assert_eq!(
            git(temporary.path(), &["rev-parse", "main"]),
            catalog.revision
        );
        assert_eq!(
            target.into_inner().err().unwrap().into_inner().last_seq(),
            0
        );
    }

    #[test]
    fn rebuild_poisoned_target_after_git_reports_persisted_boundary() {
        let (temporary, catalog, _original) = fixture();
        let request = prepare(&catalog).unwrap();
        let target_dir = temporary.path().join("lost-hub");
        let target = Mutex::new(Hub::open(&target_dir).unwrap());
        let error = SnapshotWriter::default()
            .rebuild(
                &target,
                &catalog,
                &request,
                &confirmation(&request),
                "host",
                || {
                    if git(temporary.path(), &["rev-parse", "main"]) != catalog.revision {
                        assert!(std::panic::catch_unwind(|| {
                            let _guard = target.try_lock().unwrap();
                            panic!("poison target mutex after main commit");
                        })
                        .is_err());
                    }
                    Ok(2000)
                },
            )
            .unwrap_err();
        let current = Catalog::load(temporary.path()).unwrap();
        assert_ne!(current.revision, catalog.revision);
        assert!(
            error.contains(&current.revision)
                && error.contains("已写 main")
                && error.contains("hub 锁损坏"),
            "{error}"
        );
        let target = target.into_inner().err().unwrap().into_inner();
        assert_eq!(target.last_seq(), 0);
        assert!(target.base_snapshot().is_none());
        assert!(!target_dir.join("journal.json").exists());
        assert!(snapshot_service::validate_authority(&target, &current).is_err());
    }

    #[test]
    fn recovery_persists_new_main_boundary_before_installing_and_reopens() {
        let (temporary, catalog, original) = fixture();
        let request = prepare(&catalog).unwrap();
        let target_dir = temporary.path().join("lost-hub");
        let target = Mutex::new(Hub::open(&target_dir).unwrap());
        let result = execute(
            &target,
            &catalog,
            &request,
            &confirmation(&request),
            "host",
            || Ok(2000),
        )
        .unwrap();
        assert_eq!(result.status, "rebuilt");
        assert!(result.seq > request.seq_boundary);
        assert!(result.epoch > request.epoch_boundary);
        assert_eq!(
            git(temporary.path(), &["rev-parse", "main"]),
            result.commit.unwrap()
        );
        assert!(target.lock().unwrap().last_seq() >= result.seq);
        assert!(target
            .lock()
            .unwrap()
            .events(0)
            .iter()
            .any(|event| event.kind == "snapshot.written"
                && event.data["commit"] == request.source_commit));
        assert!(target
            .lock()
            .unwrap()
            .leases(2000)
            .values()
            .all(|lease| lease.epoch > request.epoch_boundary));
        assert_eq!(
            target
                .lock()
                .unwrap()
                .receipts()
                .get(&original.lock().unwrap().events(0)[0].eid),
            Some(&1)
        );
        drop(target);
        let target = Hub::open(&target_dir).unwrap();
        let catalog = Catalog::load(temporary.path()).unwrap();
        snapshot_service::validate_authority(&target, &catalog).unwrap();
    }

    #[test]
    fn recovery_rejects_stale_confirmation_without_writing() {
        let (temporary, catalog, hub) = fixture();
        let request = prepare(&catalog).unwrap();
        let mut bad = confirmation(&request);
        bad.expires_at = 2000;
        assert!(execute(&hub, &catalog, &request, &bad, "host", || Ok(2000)).is_err());
        bad.expires_at = 10_000;
        bad.request_id = "another-request".into();
        assert!(execute(&hub, &catalog, &request, &bad, "host", || Ok(2000)).is_err());
        assert_eq!(
            git(temporary.path(), &["rev-parse", "main"]),
            catalog.revision
        );
    }

    #[test]
    fn repeated_recovery_uses_persisted_boundary_even_if_install_failed() {
        let (temporary, catalog, _original) = fixture();
        let request = prepare(&catalog).unwrap();
        let target_dir = temporary.path().join("lost-hub");
        let target = Mutex::new(Hub::open(&target_dir).unwrap());
        fs::create_dir(target_dir.join("journal.json")).unwrap();
        assert!(execute(
            &target,
            &catalog,
            &request,
            &confirmation(&request),
            "host",
            || Ok(2000)
        )
        .is_err());
        assert_eq!(target.lock().unwrap().last_seq(), 0);
        let catalog = Catalog::load(temporary.path()).unwrap();
        let next = prepare(&catalog).unwrap();
        assert!(next.seq_boundary > request.seq_boundary + (1 << 32));
        assert!(next.epoch_boundary > request.epoch_boundary + (1 << 32));
        fs::remove_dir(target_dir.join("journal.json")).unwrap();
        assert!(execute(
            &target,
            &catalog,
            &request,
            &confirmation(&request),
            "host",
            || Ok(2000)
        )
        .is_err());
        execute(
            &target,
            &catalog,
            &next,
            &confirmation(&next),
            "host",
            || Ok(2000),
        )
        .unwrap();
    }

    #[test]
    fn checked_main_defers_recovery_without_installing() {
        for dirty in [false, true] {
            let (temporary, catalog, _original) = fixture();
            git(temporary.path(), &["switch", "main"]);
            let request = prepare(&catalog).unwrap();
            let target_dir = temporary.path().join("lost-hub");
            let target = Mutex::new(Hub::open(&target_dir).unwrap());
            let path = temporary.path().join("docs/tower/state/state.json");
            if dirty {
                fs::write(&path, "user data").unwrap();
            }
            let before = fs::read(&path).unwrap();
            let error = execute(
                &target,
                &catalog,
                &request,
                &confirmation(&request),
                "host",
                || Ok(2000),
            )
            .unwrap_err();
            assert!(error.contains("Deferred"), "{error}");
            assert_eq!(target.lock().unwrap().last_seq(), 0);
            assert!(target.lock().unwrap().base_snapshot().is_none());
            assert!(!target_dir.join("journal.json").exists());
            assert_eq!(fs::read(&path).unwrap(), before);
            assert_eq!(
                git(temporary.path(), &["rev-parse", "main"]),
                catalog.revision
            );
        }
    }

    #[test]
    fn recovered_running_task_is_ready_and_projection_survives_next_snapshot() {
        let (temporary, catalog, original) = fixture();
        git(temporary.path(), &["switch", "main"]);
        fs::write(
            temporary.path().join("docs/tower/arch/A.md"),
            "---\ntype: Module\nid: A\npaths: [src/**]\n---\n",
        )
        .unwrap();
        fs::write(
            temporary.path().join("docs/tower/specs/AC.md"),
            "---\ntype: Acceptance\nid: AC\nmodule: A\ncriteria: [works]\n---\n",
        )
        .unwrap();
        git(temporary.path(), &["add", "."]);
        git(temporary.path(), &["commit", "-qm", "context"]);
        git(temporary.path(), &["switch", "-qc", "t/T-1"]);
        let catalog = Catalog::load(&catalog.root).unwrap();
        use crate::tasks::{self, TaskCommand, TaskSpec};
        let task = TaskSpec {
            id: "T-1".into(),
            title: "task".into(),
            module: "A".into(),
            touches: vec![],
            allowed_paths: vec!["src/**".into()],
            depends_on: vec![],
            acceptance: vec!["AC".into()],
            decisions: vec![],
        };
        tasks::execute(
            &mut original.lock().unwrap(),
            &catalog,
            "host",
            TaskCommand::Add { task: task.clone() },
            None,
            1100,
        )
        .unwrap();
        let mut other = task;
        other.id = "T-2".into();
        tasks::execute(
            &mut original.lock().unwrap(),
            &catalog,
            "host",
            TaskCommand::Add { task: other },
            None,
            1100,
        )
        .unwrap();
        tasks::execute(
            &mut original.lock().unwrap(),
            &catalog,
            "host",
            TaskCommand::Claim {
                id: "T-1".into(),
                harness: None,
                harness_version: None,
            },
            None,
            1100,
        )
        .unwrap();
        SnapshotWriter::default()
            .write(&original, &catalog, "host", true, || Ok(1100))
            .unwrap();
        let catalog = Catalog::load(&catalog.root).unwrap();
        let request = prepare(&catalog).unwrap();
        let target = Mutex::new(Hub::open(&temporary.path().join("lost-hub")).unwrap());
        execute(
            &target,
            &catalog,
            &request,
            &confirmation(&request),
            "host",
            || Ok(2000),
        )
        .unwrap();
        let catalog = Catalog::load(&catalog.root).unwrap();
        let state = snapshot_service::read_snapshot(&catalog)
            .unwrap()
            .unwrap()
            .snapshot;
        assert_eq!(state.projection.objects["Task"]["T-1"]["state"], "ready");
        assert!(state.projection.objects["Run"]
            .values()
            .all(|run| run["state"] == "released"));
        let status = tasks::execute(
            &mut target.lock().unwrap(),
            &catalog,
            "host",
            TaskCommand::Status,
            None,
            2000,
        )
        .unwrap();
        assert_eq!(status["tasks"].as_array().unwrap().len(), 2);
        assert!(status["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == "T-1" && task["state"] == "ready"));
        SnapshotWriter::default()
            .write(&target, &catalog, "host", true, || Ok(2100))
            .unwrap();
        let catalog = Catalog::load(&catalog.root).unwrap();
        snapshot_service::validate_authority(&target.lock().unwrap(), &catalog).unwrap();
    }
}
