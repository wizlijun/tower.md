use std::fs;
use std::os::unix::fs::PermissionsExt;
use towermd_app::cli::resolve_cli;

fn executable(path: &std::path::Path) {
    fs::write(path, b"fixture").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn bundled_cli_is_next_to_app_executable_regardless_of_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let macos = dir.path().join("Applications/Tower.md.app/Contents/MacOS");
    fs::create_dir_all(&macos).unwrap();
    let cli = macos.join("towermd");
    executable(&cli);
    assert_eq!(resolve_cli(&macos.join("towermd-app"), false).unwrap(), cli);
}

#[test]
fn missing_bundle_cli_is_tool_missing_even_when_path_has_a_towermd() {
    if let Some(exe) = std::env::var_os("TOWER_TEST_MISSING_EXE") {
        let error = resolve_cli(std::path::Path::new(&exe), false).unwrap_err();
        assert_eq!(error.code, "tool_missing");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    executable(&dir.path().join("towermd"));
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "missing_bundle_cli_is_tool_missing_even_when_path_has_a_towermd",
        ])
        .env("PATH", dir.path())
        .env(
            "TOWER_TEST_MISSING_EXE",
            dir.path().join("missing/towermd-app"),
        )
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn resolver_is_independent_of_actual_process_cwd() {
    if let Some(exe) = std::env::var_os("TOWER_TEST_APP_EXE") {
        let expected = std::path::PathBuf::from(std::env::var_os("TOWER_TEST_CLI").unwrap());
        assert_eq!(
            resolve_cli(std::path::Path::new(&exe), false).unwrap(),
            expected
        );
        return;
    }
    let bundle = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let cli = bundle.path().join("towermd");
    executable(&cli);
    for cwd in [bundle.path(), elsewhere.path()] {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "resolver_is_independent_of_actual_process_cwd"])
            .env("TOWER_TEST_APP_EXE", bundle.path().join("towermd-app"))
            .env("TOWER_TEST_CLI", &cli)
            .current_dir(cwd)
            .status()
            .unwrap();
        assert!(status.success());
    }
}

#[test]
fn development_cli_is_from_compile_time_manifest_and_host_triple() {
    let expected = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("binaries/towermd-{}", env!("TOWERMD_HOST_TRIPLE")));
    assert_eq!(
        resolve_cli(std::path::Path::new("/unrelated/anywhere/app"), true).unwrap(),
        expected
    );
}

#[test]
fn recorder_gets_fixed_argv_and_literal_docs_directory_with_null_stdin() {
    use towermd_app::cli::{run_cli, CliAction};
    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join("CLI recorder");
    fs::write(&cli, "#!/bin/sh\npwd\nprintf '<%s>\\n' \"$@\"\nif read line; then exit 9; fi\nprintf 'stderr 原文\\n' >&2\n").unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
    let docs = "文档 空格 '单引号' \"双引号\" $(touch SHOULD_NOT_EXIST)";
    let output = run_cli(
        &cli,
        dir.path(),
        CliAction::Init {
            docs_dir: docs.into(),
        },
    )
    .unwrap();
    assert_eq!(
        output.stdout,
        format!(
            "{}\n<init>\n<--docs-dir>\n<{}>\n",
            dir.path().canonicalize().unwrap().display(),
            docs
        )
    );
    assert_eq!(output.stderr, "stderr 原文\n");
    assert_eq!(output.exit_code, Some(0));
    assert!(!dir.path().join("SHOULD_NOT_EXIST").exists());
    let ensure = run_cli(&cli, dir.path(), CliAction::Ensure).unwrap();
    assert_eq!(
        ensure.stdout,
        format!(
            "{}\n<daemon>\n<ensure>\n",
            dir.path().canonicalize().unwrap().display()
        )
    );
}

#[test]
fn failed_cli_reports_real_exit_streams_and_inherited_path() {
    use towermd_app::cli::{run_cli, CliAction};
    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join("towermd");
    fs::write(
        &cli,
        "#!/bin/sh\nprintf 'init warning\\n'\nprintf 'tool_missing: validator\\n' >&2\nexit 7\n",
    )
    .unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
    let error = run_cli(&cli, dir.path(), CliAction::Ensure).unwrap_err();
    assert_eq!(error.code, "cli_failed");
    let detail = error.detail.unwrap();
    assert!(detail.contains("7"));
    assert!(detail.contains("init warning\n"));
    assert!(detail.contains("tool_missing: validator\n"));
    assert!(detail.contains(&format!("PATH={}", std::env::var("PATH").unwrap())));
    assert!(detail.contains("已运行的 daemon"));
}

#[test]
fn bundle_cli_must_be_a_regular_executable_file() {
    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join("towermd");
    fs::create_dir(&cli).unwrap();
    assert_eq!(
        resolve_cli(&dir.path().join("towermd-app"), false)
            .unwrap_err()
            .code,
        "tool_missing"
    );
    fs::remove_dir(&cli).unwrap();
    fs::write(&cli, b"not executable").unwrap();
    assert_eq!(
        resolve_cli(&dir.path().join("towermd-app"), false)
            .unwrap_err()
            .code,
        "tool_missing"
    );
}
