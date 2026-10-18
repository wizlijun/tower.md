//! Isolated Git execution and temporary index storage for the shared repository writer.
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::repo::git_timeout_env;

pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct Session {
    temporary: tempfile::TempDir,
    options: Vec<String>,
    date: String,
    draft_actor: bool,
}

impl Session {
    pub(crate) fn index_path(&self) -> PathBuf {
        self.temporary.path().join("index")
    }

    pub(crate) fn new(root: &Path) -> Result<Self, String> {
        let common = PathBuf::from(read_git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?);
        let parent = common.join("towermd");
        if let Ok(metadata) = fs::symlink_metadata(&parent) {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err("git-common-dir/towermd 不是安全目录".into());
            }
        }
        fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
        let temporary = tempfile::Builder::new()
            .prefix("snapshot-")
            .tempdir_in(parent)
            .map_err(|error| error.to_string())?;
        Self::with_temporary(root, temporary)
    }

    pub(crate) fn for_draft(root: &Path) -> Result<Self, String> {
        let mut session = Self::new(root)?;
        session.draft_actor = true;
        Ok(session)
    }

    pub(crate) fn for_network(root: &Path) -> Result<Self, String> {
        let temporary = tempfile::Builder::new()
            .prefix("towermd-origin-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        Self::with_temporary(root, temporary)
    }

    pub(crate) fn run_output(
        &self,
        root: &Path,
        args: &[&str],
    ) -> Result<std::process::Output, String> {
        #[cfg(test)]
        crate::snapshot_git::tests::observe(root, args);
        let mut options: Vec<&str> = self.options.iter().map(String::as_str).collect();
        options.extend_from_slice(args);
        crate::repo::git_timeout_env_output(
            root,
            &options,
            TIMEOUT,
            &[
                ("GIT_OPTIONAL_LOCKS", OsStr::new("0")),
                ("GIT_LITERAL_PATHSPECS", OsStr::new("1")),
                ("LC_ALL", OsStr::new("C")),
            ],
        )
    }

    fn with_temporary(root: &Path, temporary: tempfile::TempDir) -> Result<Self, String> {
        let hooks = temporary.path().join("hooks");
        fs::create_dir(&hooks).map_err(|error| error.to_string())?;
        let mut options = Vec::new();
        for option in [
            format!("core.hooksPath={}", path_text(&hooks)?),
            "core.fsmonitor=false".into(),
            "commit.gpgSign=false".into(),
            "gc.auto=0".into(),
            "maintenance.auto=false".into(),
            "core.splitIndex=false".into(),
            "core.autocrlf=false".into(),
            "core.safecrlf=false".into(),
            "push.followTags=false".into(),
            "remote.origin.mirror=false".into(),
            "push.gpgSign=false".into(),
        ] {
            options.extend(["-c".into(), option]);
        }
        let configuration = read_git(root, &["config", "--null", "--list"])?;
        for entry in configuration.split('\0') {
            let key = entry.split('\n').next().unwrap_or("");
            if key.starts_with("filter.") {
                if key.ends_with(".clean") || key.ends_with(".process") {
                    options.extend(["-c".into(), format!("{key}=")]);
                } else if key.ends_with(".required") {
                    options.extend(["-c".into(), format!("{key}=false")]);
                }
            }
        }
        let date = format!(
            "{} +0000",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_secs()
        );
        Ok(Self {
            temporary,
            options,
            date,
            draft_actor: false,
        })
    }

    pub(crate) fn run(
        &self,
        root: &Path,
        args: &[&str],
        index: Option<&Path>,
    ) -> Result<String, String> {
        #[cfg(test)]
        crate::snapshot_git::tests::observe(root, args);
        let mut options: Vec<&str> = self.options.iter().map(String::as_str).collect();
        options.extend_from_slice(args);
        let (actor, email) = if self.draft_actor {
            ("process:towermd-draft", "towermd-draft@localhost")
        } else {
            ("process:towermd-snapshot", "towermd-snapshot@localhost")
        };
        let mut environment = vec![
            ("GIT_AUTHOR_NAME", OsStr::new(actor)),
            ("GIT_AUTHOR_EMAIL", OsStr::new(email)),
            ("GIT_COMMITTER_NAME", OsStr::new(actor)),
            ("GIT_COMMITTER_EMAIL", OsStr::new(email)),
            ("GIT_AUTHOR_DATE", OsStr::new(&self.date)),
            ("GIT_COMMITTER_DATE", OsStr::new(&self.date)),
            ("GIT_OPTIONAL_LOCKS", OsStr::new("0")),
            ("GIT_LITERAL_PATHSPECS", OsStr::new("1")),
        ];
        if let Some(index) = index {
            environment.push(("GIT_INDEX_FILE", index.as_os_str()));
        }
        git_timeout_env(root, &options, TIMEOUT, &environment)
    }

    pub(crate) fn blob(&self, root: &Path, content: &str) -> Result<String, String> {
        let file = self.temporary.path().join("blob");
        fs::write(&file, content).map_err(|error| error.to_string())?;
        self.run(
            root,
            &["hash-object", "--no-filters", "-w", "--", path_text(&file)?],
            None,
        )
    }
}

fn path_text(path: &Path) -> Result<&str, String> {
    path.to_str().ok_or_else(|| "Git 路径不是 UTF-8".into())
}

pub(crate) fn read_git(root: &Path, args: &[&str]) -> Result<String, String> {
    #[cfg(test)]
    crate::snapshot_git::tests::observe(root, args);
    let mut options = vec!["-c", "core.fsmonitor=false", "-c", "core.quotePath=false"];
    options.extend_from_slice(args);
    git_timeout_env(
        root,
        &options,
        TIMEOUT,
        &[
            ("GIT_OPTIONAL_LOCKS", OsStr::new("0")),
            ("GIT_LITERAL_PATHSPECS", OsStr::new("1")),
        ],
    )
}

pub(crate) fn direct_revision(
    root: &Path,
    target: &RepositoryRef,
) -> Result<Option<String>, String> {
    let reference = target.name();
    let excluded = format!("--exclude={reference}/");
    let prefix = format!("{reference}\0");
    let exists_probe = ref_exists_supported(root)?;
    if exists_probe {
        let output = ref_output(root, &["show-ref", "--exists", reference])?;
        let absent = output.status.code() == Some(2);
        let expected_stderr = if absent {
            output.stderr.is_empty() || output.stderr == b"error: reference does not exist\n"
        } else {
            output.stderr.is_empty()
        };
        if !output.stdout.is_empty()
            || !expected_stderr
            || !matches!(output.status.code(), Some(0 | 2))
        {
            return Err(format!(
                "无法检查私有快照引用：{} {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        if absent {
            return Ok(None);
        }
    } else if read_git(root, &["symbolic-ref", "--quiet", reference]).is_ok() {
        return Err("私有快照引用是符号引用；拒绝读取或移动用户分支".into());
    }
    let records = if exists_probe {
        read_git(
            root,
            &[
                "for-each-ref",
                &excluded,
                "--format=%(refname)%00%(objectname)%00%(symref)%00%(objecttype)",
                reference,
            ],
        )
    } else {
        read_git(
            root,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(symref)",
                reference,
            ],
        )
    }?;
    let record = if exists_probe {
        records
            .split('\n')
            .find_map(|line| line.strip_prefix(&prefix))
    } else {
        records.lines().find_map(|line| line.strip_prefix(&prefix))
    };
    let Some(record) = record else {
        return if exists_probe {
            Err("私有快照引用存在但无法读取精确引用".into())
        } else {
            Ok(None)
        };
    };
    let (oid, fields) = record.split_once('\0').ok_or("无法解析私有快照引用")?;
    let (symbolic, object_type) = if exists_probe {
        let (symbolic, kind) = fields.split_once('\0').ok_or("无法解析私有快照引用类型")?;
        (symbolic, Some(kind))
    } else {
        (fields, None)
    };
    if oid.is_empty() || !symbolic.is_empty() {
        return Err("私有快照引用必须直接指向 commit".into());
    }
    let commit = match object_type {
        Some(kind) => kind == "commit",
        None => read_git(root, &["cat-file", "-t", oid])? == "commit",
    };
    if !commit {
        return Err("私有快照引用必须直接指向 commit".into());
    }
    Ok(Some(oid.into()))
}

pub(crate) fn ref_exists_supported(root: &Path) -> Result<bool, String> {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    if let Some(supported) = SUPPORTED.get() {
        return Ok(*supported);
    }
    // Probe outside the OnceLock: no initialization lock is held across Git I/O.
    let output = ref_output(root, &["show-ref", "-h"])?;
    let help_bytes = if output.stderr.is_empty() {
        &output.stdout
    } else if output.stdout.is_empty() {
        &output.stderr
    } else {
        return Err("Git show-ref 帮助输出通道异常".into());
    };
    let help = std::str::from_utf8(help_bytes).map_err(|_| "Git show-ref 帮助不是 UTF-8")?;
    if output.status.code() != Some(129) || !help.starts_with("usage: git show-ref ") {
        return Err(format!(
            "无法探测 Git show-ref 能力：{} {help}",
            output.status
        ));
    }
    let supported = help
        .split_ascii_whitespace()
        .any(|word| matches!(word, "--exists" | "--[no-]exists"));
    let _ = SUPPORTED.set(supported);
    Ok(*SUPPORTED.get().expect("capability winner installed"))
}

fn ref_output(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    #[cfg(test)]
    crate::snapshot_git::tests::observe(root, args);
    let mut options = vec!["-c", "core.fsmonitor=false", "-c", "core.quotePath=false"];
    options.extend_from_slice(args);
    crate::repo::git_timeout_env_output(
        root,
        &options,
        TIMEOUT,
        &[
            ("GIT_OPTIONAL_LOCKS", OsStr::new("0")),
            ("GIT_LITERAL_PATHSPECS", OsStr::new("1")),
            ("LC_ALL", OsStr::new("C")),
        ],
    )
}

/// A fixed writer-owned destination, never an arbitrary request ref.
pub(crate) struct RepositoryRef(String);
impl RepositoryRef {
    pub(crate) fn main() -> Self {
        Self("refs/heads/main".into())
    }
    pub(crate) fn snapshot() -> Self {
        Self("refs/towermd/snapshot".into())
    }
    pub(crate) fn draft(id: &str) -> Result<Self, String> {
        if id.is_empty()
            || id.len() > 255
            || id.starts_with('.')
            || id.ends_with('.')
            || id.ends_with(".lock")
            || id.contains("..")
            || id.contains("@{")
            || id.starts_with("publication-")
            || id.chars().any(|c| {
                c.is_control()
                    || c.is_whitespace()
                    || matches!(c, '/' | '\\' | '~' | '^' | ':' | '?' | '*' | '[')
            })
        {
            return Err("unsafe or reserved draft id".into());
        }
        Ok(Self(format!("refs/towermd/drafts/{id}")))
    }
    pub(crate) fn name(&self) -> &str {
        &self.0
    }
}

pub(crate) struct TreeFile<'a> {
    pub(crate) path: &'a str,
    pub(crate) mode: &'a str,
    pub(crate) content: &'a str,
}
impl Session {
    pub(crate) fn build_tree(
        &self,
        root: &Path,
        base: &str,
        files: &[TreeFile<'_>],
        deletions: &[String],
    ) -> Result<String, String> {
        let allowed: BTreeSet<_> = files
            .iter()
            .map(|file| file.path)
            .chain(deletions.iter().map(String::as_str))
            .collect();
        if allowed.len() != files.len() + deletions.len() {
            return Err("duplicate or conflicting tree patch paths".into());
        }
        for path in &allowed {
            crate::recovery_contract::repo_path(path.as_bytes())?;
            let mut prefix = String::new();
            let components: Vec<_> = path.split('/').collect();
            for (offset, component) in components.iter().enumerate() {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(component);
                let entry = self.run(root, &["ls-tree", "-z", base, "--", &prefix], None)?;
                if entry.is_empty() {
                    break;
                }
                let mode = entry.split_once(' ').map(|(mode, _)| mode).unwrap_or("");
                let valid = if offset + 1 == components.len() {
                    matches!(mode, "100644" | "100755")
                } else {
                    mode == "040000"
                };
                if !valid {
                    return Err(format!(
                        "快照路径 `{prefix}` 被 symlink、子模块或非预期对象占用"
                    ));
                }
            }
        }
        let index = self.index_path();
        self.run(root, &["read-tree", base], Some(&index))?;
        for file in files {
            crate::recovery_contract::file_mode(file.mode)?;
            let blob = self.blob(root, file.content)?;
            self.run(
                root,
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    file.mode,
                    &blob,
                    file.path,
                ],
                Some(&index),
            )?;
        }
        for path in deletions {
            self.run(
                root,
                &["update-index", "--force-remove", "--", path],
                Some(&index),
            )?;
        }
        let tree = self.run(root, &["write-tree"], Some(&index))?;
        self.validate_changes(root, base, &tree, &allowed)?;
        Ok(tree)
    }
    pub(crate) fn validate_changes(
        &self,
        root: &Path,
        base: &str,
        tree: &str,
        allowed: &BTreeSet<&str>,
    ) -> Result<(), String> {
        let changes = self.run(
            root,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "--no-renames",
                "-r",
                "-z",
                base,
                tree,
            ],
            None,
        )?;
        if changes
            .split('\0')
            .filter(|path| !path.is_empty())
            .any(|path| !allowed.contains(path))
        {
            return Err("快照改变了允许范围外的路径".into());
        }
        Ok(())
    }
    pub(crate) fn commit_tree(
        &self,
        root: &Path,
        tree: &str,
        parents: &[&str],
        message: &str,
    ) -> Result<String, String> {
        let mut args = vec!["commit-tree", tree];
        for parent in parents {
            args.extend(["-p", parent]);
        }
        args.extend(["-m", message]);
        self.run(root, &args, Some(&self.index_path()))
    }
    pub(crate) fn cas_ref(
        &self,
        root: &Path,
        target: &RepositoryRef,
        revision: &str,
        expected: &str,
    ) -> Result<(), String> {
        self.run(
            root,
            &[
                "update-ref",
                "--no-deref",
                target.name(),
                revision,
                expected,
            ],
            Some(&self.index_path()),
        )
        .map(|_| ())
    }
    pub(crate) fn origin_push_url(&self, root: &Path) -> Result<String, String> {
        let urls = self.run(
            root,
            &["remote", "get-url", "--push", "--all", "origin"],
            None,
        )?;
        if urls.lines().count() != 1 {
            return Err("origin 必须只有一个 push URL，无法原子更新多个远端".into());
        }
        Ok(urls)
    }
    pub(crate) fn push_commit(
        &self,
        root: &Path,
        url: &str,
        revision: &str,
        target: &RepositoryRef,
    ) -> Result<std::process::Output, String> {
        self.run_output(
            root,
            &[
                "push",
                "--porcelain",
                "--no-verify",
                "--no-follow-tags",
                "--no-signed",
                "--recurse-submodules=no",
                "--",
                url,
                &format!("{revision}:{}", target.name()),
            ],
        )
    }
}

#[cfg(test)]
pub(crate) fn save_draft_tree_for_test(
    root: &Path,
    id: &str,
    base: &str,
    files: &std::collections::BTreeMap<String, String>,
    expected: Option<&str>,
) -> Result<String, String> {
    let target = RepositoryRef::draft(id)?;
    let session = Session::for_draft(root)?;
    let tree_files: Vec<_> = files
        .iter()
        .map(|(path, content)| TreeFile {
            path,
            mode: "100644",
            content,
        })
        .collect();
    let tree = session.build_tree(root, base, &tree_files, &[])?;
    let mut parents = vec![base];
    if let Some(previous) = expected.filter(|previous| *previous != base) {
        parents.push(previous);
    }
    let revision = session.commit_tree(root, &tree, &parents, "Tower.md draft plumbing test")?;
    let old = expected
        .map(str::to_owned)
        .unwrap_or_else(|| "0".repeat(base.len()));
    session.cas_ref(root, &target, &revision, &old)?;
    Ok(revision)
}
