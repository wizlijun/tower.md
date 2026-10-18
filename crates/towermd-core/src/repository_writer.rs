//! Isolated Git execution and temporary index storage for the shared repository writer.
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::repo::git_timeout_env;

pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct Session {
    temporary: tempfile::TempDir,
    options: Vec<String>,
    date: String,
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
        let mut environment = vec![
            ("GIT_AUTHOR_NAME", OsStr::new("process:towermd-snapshot")),
            ("GIT_AUTHOR_EMAIL", OsStr::new("towermd-snapshot@localhost")),
            ("GIT_COMMITTER_NAME", OsStr::new("process:towermd-snapshot")),
            (
                "GIT_COMMITTER_EMAIL",
                OsStr::new("towermd-snapshot@localhost"),
            ),
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
