#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use towermd_core::repo::{git, git_timeout};

fn repository() -> tempfile::TempDir {
    let temporary = tempfile::tempdir().unwrap();
    git(temporary.path(), &["init", "-q"]).unwrap();
    temporary
}

#[test]
fn sleeping_alias_times_out_within_two_seconds() {
    let temporary = repository();
    let started = Instant::now();
    let result = git_timeout(
        temporary.path(),
        &["-c", "alias.delay=!sleep 10", "delay"],
        Duration::from_millis(100),
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    let error = result.unwrap_err();
    assert!(error.contains("超时"), "{error}");
    assert!(error.contains("delay"), "{error}");
    assert!(!error.contains("终止 git 进程组失败"), "{error}");
    eprintln!("100ms timeout returned after {:?}", started.elapsed());
}

#[test]
fn output_and_failure_match_existing_git_behavior() {
    let temporary = repository();
    for alias in [
        "alias.check=!printf 'retained \\n\\n'",
        "alias.check=!printf 'failure \\n\\n' >&2; exit 7",
        "alias.check=!printf '\\377'",
        "alias.check=!printf '\\377' >&2; exit 7",
    ] {
        let args = ["-c", alias, "check"];
        assert_eq!(
            git_timeout(temporary.path(), &args, Duration::from_secs(2)),
            git(temporary.path(), &args),
        );
    }
    assert_eq!(
        git_timeout(
            temporary.path(),
            &["-c", "alias.check=!printf 'retained \\n\\n'", "check"],
            Duration::from_secs(2),
        )
        .unwrap(),
        "retained \n"
    );
}

#[test]
fn drains_both_pipes_larger_than_pipe_capacity() {
    let temporary = repository();
    let output = git_timeout(
        temporary.path(),
        &[
            "-c",
            "alias.flood=!head -c 1048576 /dev/zero; head -c 1048576 /dev/zero >&2",
            "flood",
        ],
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(output.len(), 1048576);
    assert!(output.bytes().all(|byte| byte == 0));
}

#[test]
fn stdin_is_closed_and_terminal_prompts_are_disabled() {
    let temporary = repository();
    assert_eq!(
        git_timeout(
            temporary.path(),
            &[
                "-c",
                "alias.input=!if read value; then exit 9; fi; printf '%s' \"$GIT_TERMINAL_PROMPT\"",
                "input",
            ],
            Duration::from_secs(2),
        )
        .unwrap(),
        "0"
    );
}

struct OwnedProcess(Child);

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn assert_process_stopped(pid: &str) {
    let started = Instant::now();
    loop {
        let output = Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .unwrap();
        let state = String::from_utf8(output.stdout).unwrap();
        if state.trim().is_empty() || state.trim().starts_with('Z') {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "test-owned process {pid} is still running: {state}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn kills_only_its_own_group_including_inherited_pipe_holders() {
    let mut unrelated = OwnedProcess(
        Command::new("sleep")
            .arg("10")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    for ending in ["wait", "exit 0"] {
        let temporary = repository();
        let alias = format!(
            "alias.delay=!echo $$ > shell.pid; echo $PPID > git.pid; sleep 10 & echo $! > sleep.pid; {ending}"
        );
        let started = Instant::now();
        let error = git_timeout(
            temporary.path(),
            &["-c", &alias, "delay"],
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(error.contains("超时"), "{error}");
        assert!(!error.contains("进程组失败"), "{error}");
        for filename in ["shell.pid", "sleep.pid", "git.pid"] {
            let pid = fs::read_to_string(temporary.path().join(filename)).unwrap();
            assert_process_stopped(pid.trim());
        }
        let git_pid = fs::read_to_string(temporary.path().join("git.pid")).unwrap();
        let status = Command::new("ps")
            .args(["-o", "stat=", "-p", git_pid.trim()])
            .output()
            .unwrap();
        assert!(status.stdout.is_empty(), "git leader must be reaped");
        assert!(unrelated.0.try_wait().unwrap().is_none());
    }
}

#[test]
fn escaped_pipe_holder_cannot_block_return() {
    let test_name = "escaped_pipe_holder_cannot_block_return";
    match std::env::var("TOWERMD_PIPE_HOLDER_MODE").as_deref() {
        Ok("worker") => {
            let mut connection =
                TcpStream::connect(std::env::var("TOWERMD_PIPE_HOLDER_ADDRESS").unwrap()).unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let _ = connection.read(&mut [0]);
            return;
        }
        Ok("launcher") => {
            let mut worker = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test_name, "--nocapture"])
                .env("TOWERMD_PIPE_HOLDER_MODE", "worker")
                .process_group(0)
                .spawn()
                .unwrap();
            fs::write("escaped.pid", worker.id().to_string()).unwrap();
            worker.wait().unwrap();
            return;
        }
        _ => {}
    }
    let temporary = repository();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let executable = std::env::current_exe().unwrap();
    let executable = executable.to_str().unwrap().replace('\'', "'\\''");
    let alias = format!(
        "alias.escape=!TOWERMD_PIPE_HOLDER_MODE=launcher TOWERMD_PIPE_HOLDER_ADDRESS={} '{executable}' --exact {test_name} --nocapture",
        listener.local_addr().unwrap()
    );
    let started = Instant::now();
    let error = git_timeout(
        temporary.path(),
        &["-c", &alias, "escape"],
        Duration::from_millis(300),
    )
    .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(error.contains("超时"), "{error}");
    let (mut connection, _) = listener.accept().unwrap();
    connection.write_all(b"release").unwrap();
    let pid = fs::read_to_string(temporary.path().join("escaped.pid")).unwrap();
    assert_process_stopped(pid.trim());
}

#[test]
fn deadline_applies_after_both_pipes_close() {
    let temporary = repository();
    let started = Instant::now();
    let error = git_timeout(
        temporary.path(),
        &["-c", "alias.delay=!exec >/dev/null 2>&1; sleep 10", "delay"],
        Duration::from_millis(100),
    )
    .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(error.contains("超时"), "{error}");
}

#[test]
fn missing_working_directory_reports_spawn_failure() {
    let temporary = repository();
    let error = git_timeout(
        &temporary.path().join("missing"),
        &["status"],
        Duration::from_millis(100),
    )
    .unwrap_err();
    assert!(error.contains("无法运行 git"), "{error}");
}

#[test]
fn inherited_git_environment_cannot_redirect_timeout_command() {
    if let Some(root) = std::env::var_os("TOWERMD_TIMEOUT_TEST_CWD") {
        let root = PathBuf::from(root);
        assert_eq!(
            git_timeout(
                &root,
                &["rev-parse", "--show-toplevel"],
                Duration::from_secs(2)
            )
            .unwrap(),
            root.canonicalize().unwrap().to_str().unwrap()
        );
        assert!(git_timeout(
            &root,
            &["config", "--get", "test.injected"],
            Duration::from_secs(2),
        )
        .is_err());
        return;
    }
    let target = repository();
    let other = repository();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "inherited_git_environment_cannot_redirect_timeout_command",
            "--nocapture",
        ])
        .env("TOWERMD_TIMEOUT_TEST_CWD", target.path())
        .env("GIT_DIR", other.path().join(".git"))
        .env("GIT_WORK_TREE", other.path())
        .env("GIT_COMMON_DIR", other.path().join(".git"))
        .env("GIT_INDEX_FILE", other.path().join("wrong-index"))
        .env("GIT_OBJECT_DIRECTORY", other.path().join(".git/objects"))
        .env("GIT_CEILING_DIRECTORIES", target.path().parent().unwrap())
        .env("GIT_CONFIG", other.path().join(".git/config"))
        .env("GIT_CONFIG_PARAMETERS", "'test.injected=bad'")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.worktree")
        .env("GIT_CONFIG_VALUE_0", other.path())
        .env("GIT_CONFIG_KEY_999", "test.injected")
        .env("GIT_CONFIG_VALUE_999", "bad")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn network_fetch_authentication_failure_is_clear_and_finite() {
    let temporary = repository();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let remote = format!("http://{}/repo.git", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(2) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_millis(100)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_millis(100)))
                        .unwrap();
                    let mut request = [0; 4096];
                    let _ = stream.read(&mut request);
                    let _ = stream.write_all(
                        b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("{error}"),
            }
        }
        panic!("git did not contact the test server");
    });
    let started = Instant::now();
    let error = git_timeout(
        temporary.path(),
        &[
            "-c",
            "credential.helper=",
            "-c",
            "core.askPass=",
            "-c",
            "http.proxy=",
            "fetch",
            &remote,
        ],
        Duration::from_secs(1),
    )
    .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(error.contains("fetch") && error.contains("失败"), "{error}");
    assert!(error.contains("terminal prompts disabled"), "{error}");
    server.join().unwrap();
}
