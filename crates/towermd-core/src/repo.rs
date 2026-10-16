use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub fn git_timeout(dir: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    git_timeout_env(dir, args, timeout, &[])
}

pub(crate) fn git_timeout_env(
    dir: &Path,
    args: &[&str],
    timeout: Duration,
    environment: &[(&str, &std::ffi::OsStr)],
) -> Result<String, String> {
    let started = Instant::now();
    let mut command = git_command(dir, args);
    command.envs(environment.iter().copied());
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法运行 git：{error}"))?;
    let timeout_error = || format!("git {} 超时（{timeout:?}）", args.join(" "));
    let result: Result<Output, String> = (|| {
        let (sender, receiver) = mpsc::channel();
        read_pipe(child.stdout.take().unwrap(), sender.clone(), true)?;
        read_pipe(child.stderr.take().unwrap(), sender, false)?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        for _ in 0..2 {
            let (is_stdout, bytes) = receiver
                .recv_timeout(timeout.saturating_sub(started.elapsed()))
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => timeout_error(),
                    mpsc::RecvTimeoutError::Disconnected => "git 输出读取线程意外退出".to_string(),
                })?;
            let bytes = bytes.map_err(|error| format!("无法读取 git 输出：{error}"))?;
            if is_stdout {
                stdout = bytes;
            } else {
                stderr = bytes;
            }
        }
        let status = wait_with_timeout(&mut child, timeout.saturating_sub(started.elapsed()))
            .map_err(|error| format!("无法等待 git：{error}"))?
            .ok_or_else(timeout_error)?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    })();
    match result {
        Ok(output) => decode_output(args, output),
        Err(error) => {
            let cleanup = terminate_git(child);
            Err(format!("{error}{cleanup}"))
        }
    }
}

pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_command(dir, args)
        .output()
        .map_err(|error| format!("无法运行 git：{error}"))?;
    decode_output(args, output)
}

fn read_pipe(
    mut pipe: impl Read + Send + 'static,
    sender: mpsc::Sender<(bool, io::Result<Vec<u8>>)>,
    is_stdout: bool,
) -> Result<(), String> {
    thread::Builder::new()
        .name("git-output".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe.read_to_end(&mut bytes).map(|_| bytes);
            let _ = sender.send((is_stdout, result));
        })
        .map(|_| ())
        .map_err(|error| format!("无法启动 git 输出读取线程：{error}"))
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> io::Result<Option<ExitStatus>> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(None);
        }
        thread::sleep(remaining.min(Duration::from_millis(5)));
    }
}

fn kill_and_reap(mut child: Child) -> Result<(), String> {
    let _ = child.kill();
    match wait_with_timeout(&mut child, Duration::from_millis(100)) {
        Ok(Some(_)) => Ok(()),
        result => {
            let message = match result {
                Err(error) => format!("回收子进程失败：{error}"),
                _ => "回收子进程超时".to_string(),
            };
            thread::Builder::new()
                .name("git-reaper".into())
                .spawn(move || {
                    let _ = child.wait();
                })
                .map_err(|error| format!("{message}；无法启动后台回收：{error}"))?;
            Err(format!("{message}，已转后台等待"))
        }
    }
}

fn terminate_git(child: Child) -> String {
    let mut errors = Vec::new();
    #[cfg(unix)]
    {
        let signal = Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match signal {
            Ok(mut signal) => match wait_with_timeout(&mut signal, Duration::from_millis(100)) {
                Ok(Some(status)) if status.success() => {}
                result => {
                    errors.push(format!("终止 git 进程组失败：{result:?}"));
                    if let Err(error) = kill_and_reap(signal) {
                        errors.push(error);
                    }
                }
            },
            Err(error) => errors.push(format!("无法终止 git 进程组：{error}")),
        }
    }
    if let Err(error) = kill_and_reap(child) {
        errors.push(error);
    }
    if errors.is_empty() {
        String::new()
    } else {
        format!("；{}", errors.join("；"))
    }
}

fn git_command(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.env("GIT_NO_REPLACE_OBJECTS", "1");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_PREFIX",
        "GIT_GRAFT_FILE",
        "GIT_SHALLOW_FILE",
        "GIT_REPLACE_REF_BASE",
        "GIT_NAMESPACE",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        command.env_remove(name);
    }
    for (name, _) in std::env::vars_os() {
        if name.to_str().is_some_and(|name| {
            name.starts_with("GIT_CONFIG_KEY_") || name.starts_with("GIT_CONFIG_VALUE_")
        }) {
            command.env_remove(name);
        }
    }
    #[cfg(test)]
    command
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1");
    command.args(args).current_dir(dir);
    command
}

fn decode_output(args: &[&str], output: Output) -> Result<String, String> {
    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)
            .map_err(|error| format!("git 错误输出不是 UTF-8：{error}"))?;
        return Err(format!(
            "git {} 失败：{}",
            args.join(" "),
            stderr.strip_suffix('\n').unwrap_or(&stderr)
        ));
    }
    let mut stdout =
        String::from_utf8(output.stdout).map_err(|error| format!("git 输出不是 UTF-8：{error}"))?;
    if stdout.ends_with('\n') {
        stdout.pop();
    }
    Ok(stdout)
}

pub fn toplevel(dir: &Path) -> Result<PathBuf, String> {
    git(dir, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .map_err(|error| format!("{} 不在 git 仓库中或无法读取工作树：{error}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn initialize(directory: &Path) {
        let mut command = Command::new("git");
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        assert!(command
            .args(["init", "-q"])
            .current_dir(directory)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn root_path_retains_trailing_spaces_and_newlines() {
        let temporary = tempfile::tempdir().unwrap();
        for name in ["repository ", "repository\n"] {
            let root = temporary.path().join(name);
            std::fs::create_dir(&root).unwrap();
            initialize(&root);
            assert_eq!(toplevel(&root).unwrap(), root.canonicalize().unwrap());
        }
    }

    #[test]
    fn rejects_non_utf8_git_output() {
        let temporary = tempfile::tempdir().unwrap();
        initialize(temporary.path());
        std::fs::write(
            temporary.path().join(".git/config"),
            b"[test]\noutput = \xff\n",
        )
        .unwrap();
        let error = git(
            temporary.path(),
            &["config", "--local", "--get", "test.output"],
        )
        .unwrap_err();
        assert!(error.contains("UTF-8"), "{error}");
    }

    #[test]
    fn inherited_git_environment_cannot_redirect_worktree() {
        if let Some(root) = std::env::var_os("TOWERMD_TEST_CWD") {
            let root = PathBuf::from(root);
            assert_eq!(toplevel(&root).unwrap(), root.canonicalize().unwrap());
            return;
        }
        let target = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        initialize(target.path());
        initialize(other.path());
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "repo::tests::inherited_git_environment_cannot_redirect_worktree",
                "--nocapture",
            ])
            .env("TOWERMD_TEST_CWD", target.path())
            .env("GIT_DIR", other.path().join(".git"))
            .env("GIT_WORK_TREE", other.path())
            .env("GIT_COMMON_DIR", other.path().join(".git"))
            .env("GIT_INDEX_FILE", other.path().join("wrong-index"))
            .env("GIT_OBJECT_DIRECTORY", other.path().join(".git/objects"))
            .env("GIT_CEILING_DIRECTORIES", target.path().parent().unwrap())
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "core.worktree")
            .env("GIT_CONFIG_VALUE_0", other.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}
