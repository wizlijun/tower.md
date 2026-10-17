use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    G0,
    G2,
    G3,
    G5,
    G1,
    G4,
}
#[derive(Clone, Debug, PartialEq)]
pub enum GateError {
    MissingContext(String),
    Dirty,
    OperationInProgress(String),
    WrongBinding,
    StaleFence,
    NotLatestMain,
    Protected(Vec<u8>),
    OutsideTask(Vec<u8>),
    UnsupportedMode(Vec<u8>),
    InvalidObject(Vec<u8>),
    NoTests,
    ReviewBlocked(Gate),
    ProofRejected(String),
    Infrastructure(String),
}
#[derive(Clone, Debug, PartialEq)]
pub struct RawChange {
    pub old_mode: u32,
    pub new_mode: u32,
    pub old_oid: String,
    pub new_oid: String,
    pub status: u8,
    pub path: Vec<u8>,
}
pub fn parse_raw_z(raw: &[u8]) -> Result<Vec<RawChange>, GateError> {
    if raw.is_empty() {
        return Ok(vec![]);
    }
    let error = || GateError::Infrastructure("invalid raw-z diff".into());
    let fields = raw
        .strip_suffix(&[0])
        .ok_or_else(error)?
        .split(|b| *b == 0)
        .collect::<Vec<_>>();
    if fields.len() % 2 != 0 {
        return Err(error());
    }
    let mut result = Vec::new();
    let mut paths = std::collections::BTreeSet::new();
    for pair in fields.as_chunks::<2>().0 {
        let header = std::str::from_utf8(pair[0])
            .map_err(|_| error())?
            .strip_prefix(':')
            .ok_or_else(error)?;
        let h = header.split(' ').collect::<Vec<_>>();
        if h.len() != 5 || h[4].len() != 1 {
            return Err(error());
        }
        for mode in &h[..2] {
            if !matches!(*mode, "000000" | "100644" | "100755") {
                return Err(GateError::UnsupportedMode(pair[1].to_vec()));
            }
        }
        let old_mode = u32::from_str_radix(h[0], 8).map_err(|_| error())?;
        let new_mode = u32::from_str_radix(h[1], 8).map_err(|_| error())?;
        for oid in &h[2..4] {
            crate::recovery_contract::object_id(oid)
                .map_err(|_| GateError::InvalidObject(pair[1].to_vec()))?;
        }
        if h[2].len() != h[3].len()
            || (old_mode == 0) != h[2].bytes().all(|b| b == b'0')
            || (new_mode == 0) != h[3].bytes().all(|b| b == b'0')
        {
            return Err(GateError::InvalidObject(pair[1].to_vec()));
        }
        let status = h[4].as_bytes()[0];
        if !matches!(
            (status, old_mode == 0, new_mode == 0),
            (b'A', true, false) | (b'D', false, true) | (b'M' | b'T', false, false)
        ) {
            return Err(error());
        }
        crate::recovery_contract::repo_path(pair[1])
            .map_err(|_| GateError::OutsideTask(pair[1].to_vec()))?;
        if !paths.insert(pair[1]) {
            return Err(error());
        }
        result.push(RawChange {
            old_mode,
            new_mode,
            old_oid: h[2].into(),
            new_oid: h[3].into(),
            status,
            path: pair[1].to_vec(),
        });
    }
    Ok(result)
}
pub fn check_g5(
    catalog: &crate::catalog::Catalog,
    frozen: &crate::recovery_contract::finish::FrozenFinish,
    candidate: &str,
    candidate_base: &str,
) -> Result<(), GateError> {
    let root = &catalog.root;
    if frozen.project_id != catalog.config.project_id() {
        return Err(GateError::WrongBinding);
    }
    let authority = current_main(root)?;
    if authority != catalog.revision {
        return Err(GateError::NotLatestMain);
    }
    for oid in [
        &frozen.base,
        &frozen.authority,
        &authority,
        candidate,
        candidate_base,
    ] {
        commit_id(root, oid)?;
    }
    crate::recovery_contract::object_id(&frozen.tree)
        .map_err(|_| GateError::InvalidObject(frozen.tree.as_bytes().to_vec()))?;
    crate::recovery_contract::object_id(&frozen.commit)
        .map_err(|_| GateError::InvalidObject(frozen.commit.as_bytes().to_vec()))?;
    let frozen_available = match crate::repo::git_object_type(root, &frozen.commit)
        .map_err(GateError::Infrastructure)?
    {
        Some(kind) if kind == "commit" => true,
        None => false,
        Some(_) => return Err(GateError::InvalidObject(frozen.commit.as_bytes().to_vec())),
    };
    if frozen_available {
        if git_text(
            root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{}^{{tree}}", frozen.commit),
            ],
        )? != frozen.tree
        {
            return Err(GateError::ProofRejected(
                "evidence tree differs from actual frozen commit".into(),
            ));
        }
        if !ancestor(root, &frozen.authority, &frozen.commit)? {
            return Err(GateError::ProofRejected(
                "frozen task lacks original authority ancestry".into(),
            ));
        }
    }
    for (old, new) in [
        (frozen.base.as_str(), candidate_base),
        (frozen.authority.as_str(), candidate_base),
        (candidate_base, candidate),
        (candidate_base, authority.as_str()),
    ] {
        if !ancestor(root, old, new)? {
            return Err(GateError::ProofRejected(
                "unverifiable task/base ancestry".into(),
            ));
        }
    }
    let docs = catalog.config.docs_dir();
    // G5 state equivalence never grants the task permission to write state/canvas.
    let mut task_ranges = vec![(candidate_base, candidate)];
    if frozen_available {
        task_ranges.push((frozen.authority.as_str(), frozen.commit.as_str()));
    }
    for (old, new) in task_ranges {
        for change in changes(root, old, new)? {
            if state_path(docs, &change.path) {
                return Err(GateError::Protected(change.path));
            }
        }
    }
    let mut anchors = std::collections::BTreeSet::new();
    anchors.insert(frozen.base.as_str());
    anchors.insert(frozen.authority.as_str());
    for anchor in anchors {
        state_only_chain(root, docs, anchor, candidate_base)?;
    }
    if frozen_available {
        for change in changes(root, &frozen.tree, candidate)? {
            if !state_path(docs, &change.path) {
                return Err(GateError::ProofRejected(
                    "candidate differs from evidence tree outside state/canvas".into(),
                ));
            }
        }
    } else if reconstruct_evidence_tree(root, docs, candidate, &frozen.authority)? != frozen.tree {
        return Err(GateError::ProofRejected(
            "reachable candidate cannot reconstruct accepted evidence tree".into(),
        ));
    }
    if current_main(root)? != authority {
        return Err(GateError::NotLatestMain);
    }
    Ok(())
}
// Unreachable amended task commits/trees are not transported by a fresh clone. Rebuild the
// original complete tree from the reachable candidate and original authority's protected state.
// Only disposable objects and a private temporary index are written; no user ref/index is used.
fn reconstruct_evidence_tree(
    root: &std::path::Path,
    docs: &str,
    candidate: &str,
    authority: &str,
) -> Result<String, GateError> {
    let temporary = tempfile::tempdir().map_err(|e| GateError::Infrastructure(e.to_string()))?;
    let index = temporary.path().join("index");
    let run = |args: &[&str]| {
        crate::repo::git_timeout_env(
            root,
            args,
            std::time::Duration::from_secs(10),
            &[("GIT_INDEX_FILE", index.as_os_str())],
        )
        .map_err(GateError::Infrastructure)
    };
    run(&["read-tree", candidate])?;
    let state = format!("{docs}/state");
    let canvas = format!("{docs}/tower.canvas");
    let entries = |revision: &str| -> Result<Vec<(String, String, String)>, GateError> {
        let bytes = crate::repo::git_bytes_timeout(
            root,
            &[
                "ls-tree",
                "-r",
                "-z",
                "--full-tree",
                revision,
                "--",
                &format!(":(literal){state}"),
                &format!(":(literal){canvas}"),
            ],
            std::time::Duration::from_secs(10),
        )
        .map_err(GateError::Infrastructure)?;
        let mut entries = Vec::new();
        for row in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let i = row
                .iter()
                .position(|b| *b == b'\t')
                .ok_or_else(|| GateError::Infrastructure("invalid tree entry".into()))?;
            let fields = std::str::from_utf8(&row[..i])
                .map_err(|e| GateError::Infrastructure(e.to_string()))?
                .split_whitespace()
                .collect::<Vec<_>>();
            let path = std::str::from_utf8(&row[i + 1..])
                .map_err(|_| GateError::InvalidObject(row[i + 1..].to_vec()))?;
            crate::recovery_contract::repo_path(path.as_bytes())
                .map_err(|_| GateError::InvalidObject(path.as_bytes().to_vec()))?;
            if fields.len() != 3
                || fields[1] != "blob"
                || !matches!(fields[0], "100644" | "100755")
                || !state_path(docs, path.as_bytes())
            {
                return Err(GateError::UnsupportedMode(path.as_bytes().to_vec()));
            }
            entries.push((fields[0].into(), fields[2].into(), path.into()));
        }
        Ok(entries)
    };
    for (_, _, path) in entries(candidate)? {
        run(&["update-index", "--force-remove", "--", &path])?;
    }
    for (mode, oid, path) in entries(authority)? {
        run(&["update-index", "--add", "--cacheinfo", &mode, &oid, &path])?;
    }
    run(&["write-tree"])
}

fn git_text(root: &std::path::Path, args: &[&str]) -> Result<String, GateError> {
    crate::repo::git_timeout(root, args, std::time::Duration::from_secs(10))
        .map_err(GateError::Infrastructure)
}
fn commit_id(root: &std::path::Path, oid: &str) -> Result<(), GateError> {
    crate::recovery_contract::object_id(oid)
        .map_err(|_| GateError::InvalidObject(oid.as_bytes().to_vec()))?;
    if git_text(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{oid}^{{commit}}"),
        ],
    )? != oid
    {
        return Err(GateError::InvalidObject(oid.as_bytes().to_vec()));
    }
    Ok(())
}
fn ancestor(root: &std::path::Path, old: &str, new: &str) -> Result<bool, GateError> {
    let output = crate::repo::git_timeout_env_output(
        root,
        &["merge-base", "--is-ancestor", old, new],
        std::time::Duration::from_secs(10),
        &[],
    )
    .map_err(GateError::Infrastructure)?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(GateError::Infrastructure(
            "cannot inspect Git ancestry".into(),
        )),
    }
}
fn current_main(root: &std::path::Path) -> Result<String, GateError> {
    let remotes = git_text(root, &["remote"])?;
    let reference = if remotes.lines().any(|r| r == "origin") {
        "refs/remotes/origin/main"
    } else {
        "refs/heads/main"
    };
    let symbolic = crate::repo::git_timeout_env_output(
        root,
        &["symbolic-ref", "--quiet", reference],
        std::time::Duration::from_secs(10),
        &[],
    )
    .map_err(GateError::Infrastructure)?;
    match symbolic.status.code() {
        Some(1) => {}
        Some(0) => return Err(GateError::NotLatestMain),
        _ => {
            return Err(GateError::Infrastructure(
                "cannot inspect main reference".into(),
            ))
        }
    }
    git_text(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )
}
fn state_only_chain(
    root: &std::path::Path,
    docs: &str,
    old: &str,
    new: &str,
) -> Result<(), GateError> {
    // Enumerate all new ancestors and every actual parent edge, including side branches of merges.
    let history = git_text(
        root,
        &[
            "rev-list",
            "--reverse",
            "--topo-order",
            "--parents",
            new,
            "--not",
            old,
        ],
    )?;
    for line in history.lines() {
        let mut parts = line.split_whitespace();
        let commit = parts
            .next()
            .ok_or_else(|| GateError::Infrastructure("empty Git parent row".into()))?;
        let parents = parts.collect::<Vec<_>>();
        if parents.is_empty() {
            return Err(GateError::ProofRejected("incomplete base history".into()));
        }
        for parent in parents {
            for change in changes(root, parent, commit)? {
                if !state_path(docs, &change.path) {
                    return Err(GateError::ProofRejected(
                        "base advanced outside state/canvas; rebase and finish again".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn changes(root: &std::path::Path, old: &str, new: &str) -> Result<Vec<RawChange>, GateError> {
    let raw = crate::repo::git_bytes_timeout(
        root,
        &[
            "diff-tree",
            "--no-commit-id",
            "-r",
            "--raw",
            "-z",
            "--full-index",
            "--abbrev=0",
            "--no-abbrev",
            "--no-renames",
            old,
            new,
            "--",
        ],
        std::time::Duration::from_secs(10),
    )
    .map_err(GateError::Infrastructure)?;
    let changes = parse_raw_z(&raw)?;
    let mut checked = std::collections::BTreeSet::new();
    for change in &changes {
        for (mode, oid) in [
            (change.old_mode, &change.old_oid),
            (change.new_mode, &change.new_oid),
        ] {
            if mode != 0
                && checked.insert(oid)
                && git_text(root, &["cat-file", "-t", oid])? != "blob"
            {
                return Err(GateError::InvalidObject(change.path.clone()));
            }
        }
    }
    Ok(changes)
}
fn state_path(docs_dir: &str, path: &[u8]) -> bool {
    path.starts_with(format!("{docs_dir}/state/").as_bytes())
        || path == format!("{docs_dir}/tower.canvas").as_bytes()
}
