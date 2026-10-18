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
    let root = &catalog.root;
    if current_main(root)? != catalog.revision {
        return Err(GateError::NotLatestMain);
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
    if !super::ancestor(root, &catalog.revision, &commit)? {
        return Err(GateError::NotLatestMain);
    }
    let changed = changes(root, &catalog.revision, &commit)?;
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
    for change in &changed {
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
    if git_text(root, &["rev-parse", "--verify", "HEAD^{commit}"])? != commit
        || current_main(root)? != catalog.revision
    {
        return Err(GateError::NotLatestMain);
    }
    if branch(root)? != run.branch {
        return Err(GateError::WrongBinding);
    }
    clean(root)?;
    no_operation(root)?;
    Ok(CheckReport {
        commit,
        tree,
        base: catalog.revision.clone(),
        changed,
        gates: BTreeMap::from([
            ("G0".into(), true),
            ("G2".into(), true),
            ("G3".into(), false),
            ("G5".into(), true),
        ]),
    })
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
