//! Live, read-only checks over a synchronized Task/Run and authority snapshot.
use super::{changes, current_main, git_text, GateError, RawChange};
use crate::{
    catalog::{self, Catalog},
    hub_protocol::Lease,
    okf::Document,
    repo,
    tasks::{self, Run, Task},
    types,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};

struct CheckCapture {
    task: Task,
    run: Run,
    leases: BTreeMap<String, Lease>,
    session: crate::project_session::SnapshotSessionId,
}

/// Capture and recheck actual Session state in short continuations. Git I/O is
/// outside the Session lock, so unrelated status and renewals remain available.
pub fn check_shared(
    session: &std::sync::Mutex<crate::project_session::ProjectSession>,
    catalog: &Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    host: &str,
    run_override: Option<&str>,
    clock: impl Fn() -> Result<u64, String>,
) -> Result<CheckReport, GateError> {
    let now = clock().map_err(GateError::Infrastructure)?;
    let captured = capture_shared(session, catalog, inputs, host, run_override, now)?;
    let report = check(
        catalog,
        &captured.task,
        &captured.run,
        host,
        &captured.leases,
        now,
    )?;
    let finished_at = clock().map_err(GateError::Infrastructure)?;
    if finished_at < now {
        return Err(GateError::Infrastructure(
            "clock moved backwards during check".into(),
        ));
    }
    let current = capture_shared(session, catalog, inputs, host, run_override, finished_at)?;
    let identity = |task: &Task, run: &Run| {
        serde_json::to_value((task, run))
            .map_err(|error| GateError::Infrastructure(error.to_string()))
    };
    if current.session != captured.session
        || identity(&current.task, &current.run)? != identity(&captured.task, &captured.run)?
    {
        return Err(GateError::WrongBinding);
    }
    for fence in &captured.run.leases {
        if current
            .leases
            .get(&fence.name)
            .is_none_or(|lease| lease.fence() != *fence || lease.expires_at <= finished_at)
        {
            return Err(GateError::StaleFence);
        }
    }
    Ok(report)
}

fn capture_shared(
    session: &std::sync::Mutex<crate::project_session::ProjectSession>,
    catalog: &Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    host: &str,
    run_override: Option<&str>,
    now: u64,
) -> Result<CheckCapture, GateError> {
    crate::project_session::ProjectSession::continue_shared(session, catalog, inputs, now, |live| {
        // projection() checks that recovery has not retired this Session.
        live.projection()?;
        let (task, run) = live.tasks.check_context(catalog, host, run_override)?;
        let batch = live
            .hub
            .subscribe(live.cursor, now)
            .map_err(|error| error.to_string())?;
        if !batch.events.is_empty() || batch.head != live.head {
            return Err("check context changed before capture".into());
        }
        Ok(CheckCapture {
            task,
            run,
            leases: batch.head.leases,
            session: live.snapshot_session_id(),
        })
    })
    .map_err(|error| GateError::ProofRejected(error.to_string()))?
    .map_err(GateError::MissingContext)
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckReport {
    pub commit: String,
    pub tree: String,
    pub base: String,
    pub gates: BTreeMap<String, bool>,
    pub changed: Vec<RawChange>,
}

/// The caller supplies the current Session's Task, Run and lease projection.
/// Dependency completion and pending rechecks must also be checked by that
/// Session before accepting an operation. This result is never a capability.
pub fn check(
    catalog: &Catalog,
    task: &Task,
    run: &Run,
    host: &str,
    leases: &BTreeMap<String, Lease>,
    now_ms: u64,
) -> Result<CheckReport, GateError> {
    check_at(catalog, task, run, host, leases, now_ms, None)
}

/// Only an already frozen finish may retain its original Task HEAD while
/// current main advances through the existing G5 state/canvas exception.
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_frozen(
    catalog: &Catalog,
    task: &Task,
    run: &Run,
    host: &str,
    leases: &BTreeMap<String, Lease>,
    now_ms: u64,
    frozen: &crate::recovery_contract::finish::FrozenFinish,
) -> Result<CheckReport, GateError> {
    check_at(catalog, task, run, host, leases, now_ms, Some(frozen))
}

#[allow(clippy::too_many_arguments)]
fn check_at(
    catalog: &Catalog,
    task: &Task,
    run: &Run,
    host: &str,
    leases: &BTreeMap<String, Lease>,
    now_ms: u64,
    frozen: Option<&crate::recovery_contract::finish::FrozenFinish>,
) -> Result<CheckReport, GateError> {
    let root = &catalog.root;
    if current_main(root)? != catalog.revision {
        return Err(GateError::ProofRejected(
            "check authority changed before reading the Task commit".into(),
        ));
    }
    tasks::validate_task(&task.spec, catalog).map_err(GateError::MissingContext)?;
    if task.object_type != "Task"
        || run.object_type != "Run"
        || !matches!(task.state.as_str(), "running" | "verifying" | "held")
        || !matches!(run.state.as_str(), "running" | "verifying" | "held")
        || task
            .extra
            .get("needs_recheck")
            .is_some_and(|value| value != &serde_json::Value::Bool(false))
    {
        return Err(GateError::MissingContext(
            "task is not executable or requires recheck".into(),
        ));
    }
    if run.task != task.spec.id
        || run.host != host
        || host.is_empty()
        || run.worktree != root.to_string_lossy()
        || run.branch != format!("t/{}", task.spec.id)
        || catalog.branch != run.branch
        || repo::toplevel(root).map_err(GateError::Infrastructure)? != *root
        || branch(root)? != run.branch
    {
        return Err(GateError::WrongBinding);
    }
    let modules: BTreeSet<_> = std::iter::once(&task.spec.module)
        .chain(task.spec.touches.iter())
        .collect();
    let expected: BTreeSet<_> = modules.iter().map(|id| format!("module:{id}")).collect();
    let names: BTreeSet<_> = run.leases.iter().map(|fence| fence.name.clone()).collect();
    if names != expected
        || names.len() != run.leases.len()
        || run.leases.iter().any(|fence| {
            fence.holder != run.id
                || fence.epoch == 0
                || leases.get(&fence.name).is_none_or(|lease| {
                    lease.name != fence.name
                        || lease.holder != fence.holder
                        || lease.epoch != fence.epoch
                        || lease.expires_at <= now_ms
                })
        })
    {
        return Err(GateError::StaleFence);
    }
    clean(root)?;
    no_operation(root)?;
    let commit = git_text(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let tree = git_text(
        root,
        &["rev-parse", "--verify", &format!("{commit}^{{tree}}")],
    )?;
    let task_authority = if let Some(frozen) = frozen {
        if frozen.project_id != catalog.config.project_id()
            || frozen.host != host
            || frozen.task_id != task.spec.id
            || frozen.run_id != run.id
            || frozen.module_id != task.spec.module
            || frozen.fences != run.leases
            || frozen.root != catalog.root
            || frozen.common_dir != catalog.common_dir
        {
            return Err(GateError::WrongBinding);
        }
        if frozen.commit != commit || frozen.tree != tree {
            return Err(GateError::ProofRejected(
                "actual Task HEAD/tree differs from the frozen verification".into(),
            ));
        }
        super::check_finish_authority(catalog, frozen)?;
        frozen.authority.as_str()
    } else {
        if !super::ancestor(root, &catalog.revision, &commit)? {
            return Err(GateError::NotLatestMain);
        }
        catalog.revision.as_str()
    };
    let changed = changes(root, task_authority, &commit)?;
    check_paths(catalog, task, &changed)?;
    if git_text(root, &["rev-parse", "--verify", "HEAD^{commit}"])? != commit
        || current_main(root)? != catalog.revision
    {
        return Err(GateError::ProofRejected(
            "check HEAD or authority changed during verification".into(),
        ));
    }
    if branch(root)? != run.branch {
        return Err(GateError::WrongBinding);
    }
    clean(root)?;
    no_operation(root)?;
    Ok(CheckReport {
        commit,
        tree,
        base: frozen.map_or_else(|| catalog.revision.clone(), |frozen| frozen.base.clone()),
        changed,
        gates: BTreeMap::from([
            ("G0".into(), true),
            ("G2".into(), true),
            ("G3".into(), false),
            ("G5".into(), true),
        ]),
    })
}

/// Reuse the identical G2 path inspector for immutable historical diagnostics.
/// It does not run commands, inspect current HEAD or grant a gate capability.
pub(crate) fn check_paths_at(
    catalog: &Catalog,
    task: &Task,
    authority: &str,
    commit: &str,
) -> Result<Vec<RawChange>, GateError> {
    let changed = changes(&catalog.root, authority, commit)?;
    check_paths(catalog, task, &changed)?;
    Ok(changed)
}
fn check_paths(catalog: &Catalog, task: &Task, changed: &[RawChange]) -> Result<(), GateError> {
    let root = &catalog.root;
    let modules: BTreeSet<_> = std::iter::once(&task.spec.module)
        .chain(task.spec.touches.iter())
        .collect();
    let mut module_paths = Vec::<String>::new();
    for id in modules {
        module_paths.extend(
            serde_yaml::from_value::<Vec<String>>(
                catalog.objects[id]
                    .get("paths")
                    .cloned()
                    .unwrap_or_default(),
            )
            .map_err(|error| GateError::MissingContext(error.to_string()))?,
        );
    }
    let mut inspected = BTreeSet::new();
    for change in changed {
        let path = std::str::from_utf8(&change.path)
            .map_err(|_| GateError::OutsideTask(change.path.clone()))?;
        if protected_path(catalog, path) {
            return Err(GateError::Protected(change.path.clone()));
        }
        // Both versions matter: deletion or changing the layer cannot remove
        // the authority object's protection. Reuse the ordinary metadata rule.
        for (mode, oid) in [
            (change.old_mode, &change.old_oid),
            (change.new_mode, &change.new_oid),
        ] {
            if mode != 0 && inspected.insert(oid) {
                let raw = repo::git_bytes_timeout(
                    root,
                    &["cat-file", "blob", oid],
                    Duration::from_secs(10),
                )
                .map_err(GateError::Infrastructure)?;
                if raw.starts_with(b"---\n") || raw.starts_with(b"---\r\n") {
                    let text = std::str::from_utf8(&raw)
                        .map_err(|_| GateError::InvalidObject(change.path.clone()))?;
                    match Document::parse(text) {
                        Ok(document) if types::layer_of(&document).is_protected() => {
                            return Err(GateError::Protected(change.path.clone()))
                        }
                        Ok(_) => {}
                        Err(_) if catalog::ordinary_frontmatter(text) => {}
                        Err(_) => return Err(GateError::InvalidObject(change.path.clone())),
                    }
                }
            }
        }
        if !task
            .spec
            .allowed_paths
            .iter()
            .any(|pattern| path_matches(pattern, path))
            || !module_paths
                .iter()
                .any(|pattern| path_matches(pattern, path))
        {
            return Err(GateError::OutsideTask(change.path.clone()));
        }
    }
    Ok(())
}

fn branch(root: &Path) -> Result<String, GateError> {
    git_text(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map_err(|_| GateError::WrongBinding)
}
fn clean(root: &Path) -> Result<(), GateError> {
    let status = repo::git_bytes_timeout(
        root,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
        ],
        Duration::from_secs(10),
    )
    .map_err(GateError::Infrastructure)?;
    if status.is_empty() {
        Ok(())
    } else {
        Err(GateError::Dirty)
    }
}
fn no_operation(root: &Path) -> Result<(), GateError> {
    for name in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
        "sequencer",
    ] {
        let path = git_text(root, &["rev-parse", "--git-path", name])?;
        match std::fs::symlink_metadata(root.join(path)) {
            Ok(_) => return Err(GateError::OperationInProgress(name.into())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(GateError::Infrastructure(error.to_string())),
        }
    }
    Ok(())
}
fn protected_path(catalog: &Catalog, path: &str) -> bool {
    let path = path.to_lowercase();
    if path
        .split('/')
        .any(|part| matches!(part, ".git" | ".towermd"))
    {
        return true;
    }
    let within = |prefix: &str| {
        path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|tail| tail.starts_with('/'))
    };
    [
        "intents",
        "arch",
        "specs",
        "baselines",
        "state",
        "tower.canvas",
    ]
    .iter()
    .any(|name| within(&format!("{}/{name}", catalog.config.docs_dir()).to_lowercase()))
        || catalog
            .protected_paths
            .iter()
            .any(|path| within(&path.to_lowercase()))
}

// The authorized subset is already validated by Catalog. Match that finite
// language using path segments; '*' and '?' never cross '/', '**' does.
fn path_matches(pattern: &str, path: &str) -> bool {
    let pattern: Vec<_> = pattern.split('/').collect();
    let path: Vec<_> = path.split('/').collect();
    let mut previous = vec![false; path.len() + 1];
    previous[0] = true;
    for (part, segment) in pattern.iter().enumerate() {
        let mut next = vec![false; path.len() + 1];
        for index in 0..=path.len() {
            if *segment == "**" {
                // A trailing '/**' authorizes descendants, never replacing
                // the directory prefix itself with a file. Interior '**'
                // can match zero directories (src/**/a.rs includes src/a.rs).
                next[index] = if part + 1 == pattern.len() {
                    index > 0 && (previous[index - 1] || next[index - 1])
                } else {
                    previous[index] || index > 0 && next[index - 1]
                };
            } else if index > 0 {
                next[index] = previous[index - 1] && segment_matches(segment, path[index - 1]);
            }
        }
        previous = next;
    }
    previous[path.len()]
}
fn segment_matches(pattern: &str, text: &str) -> bool {
    let text: Vec<_> = text.chars().collect();
    let mut previous = vec![false; text.len() + 1];
    previous[0] = true;
    for ch in pattern.chars() {
        let mut next = vec![false; text.len() + 1];
        for index in 0..=text.len() {
            next[index] = if ch == '*' {
                previous[index] || index > 0 && next[index - 1]
            } else {
                index > 0 && previous[index - 1] && (ch == '?' || ch == text[index - 1])
            };
        }
        previous = next;
    }
    previous[text.len()]
}

/// CLI/process diagnostic producer. Pure `check_shared` remains unchanged;
/// reads never reach this outbox path. Stable errors are retained while the
/// final source/Session proof is attempted, and remain hard errors afterward.
pub fn check_recorded_shared(
    session: &std::sync::Mutex<crate::project_session::ProjectSession>,
    catalog: &Catalog,
    host: &str,
    run_override: Option<&str>,
    outbox: &mut crate::recovery_outbox::RecoveryOutbox,
    clock: impl Fn() -> Result<u64, String>,
) -> Result<CheckReport, GateError> {
    recorded_check_shared(session, catalog, host, run_override, outbox, &clock)
        .map(|checked| checked.report)
}

// Private, single-call observation of a real check and its durable diagnostic.
// It cannot be deserialized, supplied by a client, or grant execution authority.
struct RecordedCheck {
    report: CheckReport,
    captured: CheckCapture,
    actual: crate::hub_protocol::Event,
    checked_ms: u64,
}

fn recorded_check_shared(
    session: &std::sync::Mutex<crate::project_session::ProjectSession>,
    catalog: &Catalog,
    host: &str,
    run_override: Option<&str>,
    outbox: &mut crate::recovery_outbox::RecoveryOutbox,
    clock: &impl Fn() -> Result<u64, String>,
) -> Result<RecordedCheck, GateError> {
    use crate::{baseline_current::current_baseline, task_check_recorded as diagnostic};
    let inputs = outbox.read_inputs().map_err(GateError::Infrastructure)?;
    let now = clock().map_err(GateError::Infrastructure)?;
    let captured = capture_shared(session, catalog, &inputs, host, run_override, now)?;
    let source = checked_source(catalog)?;
    let result = check(
        catalog,
        &captured.task,
        &captured.run,
        host,
        &captured.leases,
        now,
    );
    let rejection = match &result {
        Ok(_) => None,
        Err(error) => match diagnostic::Rejection::from_error(error) {
            Some(reason) => Some(reason),
            None => return Err(error.clone()),
        },
    };
    let verified = (|| {
        let end = clock().map_err(GateError::Infrastructure)?;
        if end < now || checked_source(catalog)? != source {
            return Err(GateError::ProofRejected(
                "check source changed during diagnostic proof".into(),
            ));
        }
        let current = capture_shared(session, catalog, &inputs, host, run_override, end)?;
        if current.session != captured.session
            || serde_json::to_value((&current.task, &current.run))
                .map_err(|e| GateError::Infrastructure(e.to_string()))?
                != serde_json::to_value((&captured.task, &captured.run))
                    .map_err(|e| GateError::Infrastructure(e.to_string()))?
        {
            return Err(GateError::WrongBinding);
        }
        for fence in &captured.run.leases {
            if current
                .leases
                .get(&fence.name)
                .is_none_or(|lease| lease.fence() != *fence || lease.expires_at <= end)
            {
                return Err(GateError::StaleFence);
            }
        }
        let baseline = current_baseline(catalog, &inputs)
            .map_err(GateError::Infrastructure)?
            .ok_or_else(|| {
                GateError::MissingContext("check diagnostic requires approved baseline".into())
            })?;
        let existing = diagnostic::known_originals(session, catalog, &inputs, &captured.run, end)
            .map_err(GateError::Infrastructure)?;
        if checked_source(catalog)? != source {
            return Err(GateError::ProofRejected(
                "check source changed while proving original diagnostics".into(),
            ));
        }
        // Git proof is complete before the final short Session continuation.
        let stored = diagnostic::record_shared(
            session,
            catalog,
            outbox,
            &inputs,
            &captured.task,
            &captured.run,
            &captured.session,
            &source.0,
            &source.1,
            &baseline.baseline,
            rejection,
            &existing,
            end,
            clock,
        )
        .map_err(GateError::Infrastructure)?;
        if stored.stale {
            return Err(GateError::StaleFence);
        }
        Ok(stored)
    })();
    match result {
        Err(original) => Err(original),
        Ok(report) => {
            let actual = verified?;
            let original = crate::recovery_outbox::original_of(&actual);
            crate::recovery_outbox::validate_actual(&original, &actual)
                .map_err(GateError::Infrastructure)?;
            let record = diagnostic::decode(&original).map_err(GateError::Infrastructure)?;
            if actual.stale
                || actual.legacy
                || actual.obj != record.evidence.id
                || record.evidence.result != crate::recovery_contract::recorded::GateResult::Passed
            {
                return Err(GateError::WrongBinding);
            }
            Ok(RecordedCheck {
                report,
                captured,
                actual,
                checked_ms: now,
            })
        }
    }
}

impl RecordedCheck {
    fn finish_observation(
        self,
        session: &std::sync::Mutex<crate::project_session::ProjectSession>,
        catalog: &Catalog,
        host: &str,
        outbox: &crate::recovery_outbox::RecoveryOutbox,
        clock: &impl Fn() -> Result<u64, String>,
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
        let inputs = outbox.read_inputs().map_err(GateError::Infrastructure)?;
        let now = clock().map_err(GateError::Infrastructure)?;
        let accepted_at = crate::hub::event_time(&self.actual)
            .map_err(|error| GateError::Infrastructure(error.to_string()))?;
        if now < self.checked_ms || now < accepted_at {
            return Err(GateError::ProofRejected(
                "recorded check clock moved backwards".into(),
            ));
        }
        // This early observation is context only. The caller retains its
        // unconditional full check before creating any new finish admission.
        let end = now;
        let current = capture_shared(
            session,
            catalog,
            &inputs,
            host,
            Some(&self.captured.run.id),
            end,
        )?;
        let mut expected_run = self.captured.run;
        expected_run
            .extra
            .insert("last_check".into(), serde_json::json!(self.actual.obj));
        if current.session != self.captured.session
            || serde_json::to_value((&current.task, &current.run))
                .map_err(|error| GateError::Infrastructure(error.to_string()))?
                != serde_json::to_value((&self.captured.task, &expected_run))
                    .map_err(|error| GateError::Infrastructure(error.to_string()))?
        {
            return Err(GateError::WrongBinding);
        }
        for fence in &expected_run.leases {
            if current
                .leases
                .get(&fence.name)
                .is_none_or(|lease| lease.fence() != *fence || lease.expires_at <= end)
            {
                return Err(GateError::StaleFence);
            }
        }
        Ok((current.task, current.run, current.session, self.report, end))
    }
}

pub(crate) fn check_recorded_finish_shared(
    session: &std::sync::Mutex<crate::project_session::ProjectSession>,
    catalog: &Catalog,
    host: &str,
    run_override: Option<&str>,
    outbox: &mut crate::recovery_outbox::RecoveryOutbox,
    clock: impl Fn() -> Result<u64, String>,
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
    recorded_check_shared(session, catalog, host, run_override, outbox, &clock)?
        .finish_observation(session, catalog, host, outbox, &clock)
}

#[cfg(test)]
#[path = "task_check_finish_tests.rs"]
mod finish_tests;

fn checked_source(catalog: &Catalog) -> Result<(String, String), GateError> {
    if current_main(&catalog.root)? != catalog.revision {
        return Err(GateError::ProofRejected("check authority changed".into()));
    }
    if branch(&catalog.root)? != catalog.branch {
        return Err(GateError::WrongBinding);
    }
    clean(&catalog.root)?;
    no_operation(&catalog.root)?;
    let commit = git_text(&catalog.root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let tree = git_text(
        &catalog.root,
        &["rev-parse", "--verify", &format!("{commit}^{{tree}}")],
    )?;
    Ok((commit, tree))
}
