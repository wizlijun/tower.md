use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tempfile::TempDir;
use towermd_core::{
    catalog::Catalog,
    coldstart::{capture_scan_input, prepare_scan, ScanInput, ScanOptions},
    coldstart_verification::{aggregate_verification, verification_commands},
    config::Config,
    recovery_contract::{
        finish::EvidenceResult,
        verification::{
            commands_hash, report_hash, CommandExecution, VerificationCommand, VerificationReport,
        },
    },
    repo,
    verifier::{verify, VerificationRequest},
};

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = vec![
        "-c",
        "user.name=codex/gpt-6",
        "-c",
        "user.email=codex@towermd.invalid",
        "-c",
        "commit.gpgsign=false",
    ];
    command.extend_from_slice(args);
    repo::git(root, &command).unwrap()
}
fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}
fn fixture(makefile: bool) -> (TempDir, ScanInput, Value) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-b", "main"]);
    write(
        root.path(),
        ".towermd/config.yaml",
        &Config::new("project", "docs/tower").unwrap().to_yaml(),
    );
    for path in [
        "alpha/a.py",
        "beta/b.py",
        "gamma/c.py",
        "worker/w.py",
        "ui/index.js",
    ] {
        write(root.path(), path, "# committed source\n");
    }
    write(
        root.path(),
        "worker/pyproject.toml",
        "[project]\nname = 'worker'\n",
    );
    write(
        root.path(),
        "ui/package.json",
        r#"{"scripts":{"test":"echo pending > pending-marker"}}"#,
    );
    write(root.path(), "docs/tower/user.md", "user documentation\n");
    write(
        root.path(),
        "sample.txt",
        "real nonempty committed assertion\n",
    );
    if makefile {
        write(root.path(), "Makefile", "test:\n\t@test -s sample.txt\n\t@printf 'coldstart-real-test-marker\\n'\n\t@printf 'test\\n' >> aggregation-count\n\t@test $$(wc -l < aggregation-count) -eq 2\ncheck:\n\t@test -s sample.txt\n\t@printf 'coldstart-real-check-marker\\n'\n\t@printf 'check\\n' >> aggregation-count\n\t@test $$(wc -l < aggregation-count) -eq 1\n");
    }
    git(root.path(), &["add", "."]);
    git(root.path(), &["commit", "-qm", "fixture"]);
    let input = capture_scan_input(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    let scan = prepare_scan(&input).unwrap();
    (root, input, scan)
}
fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}
fn command_set(scan: &Value) -> Vec<VerificationCommand> {
    serde_json::from_value(scan["verification_commands"].clone()).unwrap()
}
fn report(input: &ScanInput, scan: &Value) -> VerificationReport {
    let directory = input
        .common_dir
        .join("towermd/cache/verification/report-fixture");
    VerificationReport {
        commit: input.revision.clone(),
        tree: input.tree.clone(),
        executions: command_set(scan)
            .into_iter()
            .enumerate()
            .map(|(index, command)| CommandExecution {
                command,
                result: "passed".into(),
                exit_code: Some(0),
                timed_out: false,
                stdout_log: directory
                    .join(format!("{index}-stdout.log"))
                    .to_str()
                    .unwrap()
                    .into(),
                stderr_log: directory
                    .join(format!("{index}-stderr.log"))
                    .to_str()
                    .unwrap()
                    .into(),
                stdout_sha256: "a".repeat(64),
                stderr_sha256: "b".repeat(64),
                diagnostic: None,
            })
            .collect(),
    }
}
fn module<'a>(scan: &'a Value, root: &str) -> &'a Value {
    scan["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["roots"].as_array().unwrap().iter().any(|r| r == root))
        .unwrap()
}

#[test]
fn commands_preserve_scanner_order_and_shared_deduplication() {
    let (_root, input, scan) = fixture(true);
    let commands = verification_commands(&input, &scan).unwrap();
    assert_eq!(commands, command_set(&scan));
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].argv, ["make", "check"]);
    assert_eq!(commands[1].argv, ["make", "test"]);
    assert_eq!(
        commands_hash(&commands).unwrap(),
        commands_hash(&command_set(&scan)).unwrap()
    );
}

#[test]
fn no_tests_candidates_have_exact_module_paths_and_honest_reasons() {
    let (_root, input, scan) = fixture(false);
    let report = report(&input, &scan);
    let results = aggregate_verification(&input, &scan, &report).unwrap();
    assert_eq!(results.len(), scan["modules"].as_array().unwrap().len());
    for result in results {
        assert_eq!(result.result, EvidenceResult::NoTests);
        assert!(result.execution_indices.is_empty());
        assert!(!result.diagnostics.is_empty());
        let candidate = scan["candidate_tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["module"] == result.module)
            .unwrap();
        let module = scan["modules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == result.module)
            .unwrap();
        assert_eq!(candidate["allowed_paths"], module["paths"]);
        assert_eq!(candidate["status"], "candidate");
        assert!(candidate["title"].as_str().unwrap().len() > 1);
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d["reason"] == candidate["reason"]));
        assert!(candidate.get("acceptance").is_none());
    }
    assert!(scan["candidate_tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["reason"].as_str().unwrap().contains("未发现")));
    assert!(scan["candidate_tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["reason"].as_str().unwrap().contains("待确认")));
    assert!(scan["candidate_tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["reason"].as_str().unwrap().contains("不支持")));
}

#[test]
fn scanner_candidates_suggest_actual_paths_without_claiming_absent_tests() {
    let (_root, _input, scan) = fixture(false);
    for task in scan["candidate_tasks"].as_array().unwrap() {
        let module = scan["modules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|module| module["id"] == task["module"])
            .unwrap();
        assert_eq!(task["allowed_paths"], module["paths"]);
        assert!(task["reason"]
            .as_str()
            .unwrap()
            .contains("不是测试通过证据"));
    }
}

#[test]
fn nonempty_all_successful_commands_pass_and_keep_original_report_references() {
    let (_root, input, scan) = fixture(true);
    let report = report(&input, &scan);
    let results = aggregate_verification(&input, &scan, &report).unwrap();
    assert_eq!(results.len(), scan["modules"].as_array().unwrap().len());
    for root in ["alpha", "beta", "gamma"] {
        let result = results
            .iter()
            .find(|r| r.module == module(&scan, root)["id"])
            .unwrap();
        assert_eq!(result.result, EvidenceResult::Passed);
        assert!(result.shared);
        assert_eq!(result.execution_indices, [0, 1]);
        assert_eq!(result.report_hash, report_hash(&report).unwrap());
        assert_eq!(
            result.validation_plan,
            scan["validation_plans"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["module"] == result.module)
                .unwrap()
                .clone()
        );
    }
    for root in ["worker", "ui"] {
        let result = results
            .iter()
            .find(|r| r.module == module(&scan, root)["id"])
            .unwrap();
        assert_eq!(result.result, EvidenceResult::NoTests);
        assert!(!result.shared);
        assert!(!result.diagnostics.is_empty());
    }
}

#[test]
fn any_failed_execution_fails_every_referencing_module_and_retains_diagnostic() {
    let (_root, input, scan) = fixture(true);
    for (exit_code, timed_out, diagnostic) in [
        (Some(7), false, "command exit failure"),
        (None, false, "spawn failed: missing tool"),
        (None, true, "command timeout"),
        (Some(0), false, "cancelled during command execution"),
    ] {
        let mut report = report(&input, &scan);
        report.executions[1].result = "failed".into();
        report.executions[1].exit_code = exit_code;
        report.executions[1].timed_out = timed_out;
        report.executions[1].diagnostic = Some(diagnostic.into());
        let results = aggregate_verification(&input, &scan, &report).unwrap();
        assert_eq!(
            results
                .iter()
                .filter(|r| r.result == EvidenceResult::Failed)
                .count(),
            3
        );
        for result in results
            .iter()
            .filter(|r| r.result == EvidenceResult::Failed)
        {
            assert!(result
                .diagnostics
                .iter()
                .any(|d| d["reason"] == diagnostic && d["execution_index"] == 1));
        }
    }
}

#[test]
fn rejects_incomplete_extra_duplicate_reordered_and_inconsistent_reports() {
    let (_root, input, scan) = fixture(true);
    let original = report(&input, &scan);
    let mut rejected = Vec::new();
    for variant in 0..17 {
        let mut report = original.clone();
        match variant {
            0 => {
                report.executions.pop();
            }
            1 => report.executions.push(report.executions[0].clone()),
            2 => report.executions[1] = report.executions[0].clone(),
            3 => report.executions.swap(0, 1),
            4 => report.commit = "a".repeat(40),
            5 => report.tree = "b".repeat(40),
            6 => report.executions[0].exit_code = Some(1),
            7 => report.executions[0].exit_code = None,
            8 => report.executions[0].timed_out = true,
            9 => report.executions[0].result = "no_tests".into(),
            10 => report.executions[0].stdout_sha256 = "invalid".into(),
            11 => report.executions[0].stdout_log = "/tmp/unrelated.log".into(),
            12 => report.executions[0].exit_code = Some(-1),
            13 => report.executions[0].result = "Failed".into(),
            14 => report.executions[1].stderr_log = report.executions[0].stderr_log.clone(),
            15 => {
                report.executions[1].stdout_log = input
                    .common_dir
                    .join("towermd/cache/verification/other/1-stdout.log")
                    .to_str()
                    .unwrap()
                    .into()
            }
            16 => report.executions[0].command.cwd = "./".into(),
            _ => unreachable!(),
        }
        rejected.push(aggregate_verification(&input, &scan, &report).is_err());
    }
    assert_eq!(
        rejected,
        vec![true; 17],
        "each report rejection variant must be checked"
    );
}

#[test]
fn rejects_scan_identity_module_plan_and_global_union_drift() {
    let (_root, input, scan) = fixture(true);
    let mut rejected = Vec::new();
    for variant in 0..15 {
        let mut scan = scan.clone();
        match variant {
            0 => scan["revision"] = json!("a".repeat(40)),
            1 => scan["tree"] = json!("a".repeat(40)),
            2 => scan["project_id"] = json!("other"),
            3 => {
                scan["validation_plans"].as_array_mut().unwrap().pop();
            }
            4 => scan["validation_plans"][1] = scan["validation_plans"][0].clone(),
            5 => scan["validation_plans"][0]["tree"] = json!("a".repeat(40)),
            6 => {
                scan["verification_commands"].as_array_mut().unwrap().pop();
            }
            7 => scan["verification_commands"]
                .as_array_mut()
                .unwrap()
                .swap(0, 1),
            8 => {
                let duplicate = scan["verification_commands"][0].clone();
                scan["verification_commands"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            9 => scan["modules"][1]["id"] = scan["modules"][0]["id"].clone(),
            10 => {
                for plan in scan["validation_plans"].as_array_mut().unwrap() {
                    for command in plan["commands"].as_array_mut().unwrap() {
                        command["requires_confirmation"] = json!(true);
                    }
                }
            }
            11 => scan["validation_plans"][0]["module"] = json!("unknown"),
            12 => scan["options"]["history_limit"] = json!(1),
            13 => scan["algorithm"] = json!("other"),
            14 => scan["history"]["shallow"] = json!(true),
            _ => unreachable!(),
        }
        rejected.push(verification_commands(&input, &scan).is_err());
    }
    assert_eq!(
        rejected,
        vec![true; 15],
        "each scan rejection variant must be checked"
    );
}

#[test]
fn real_owned_verifier_runs_shared_commands_once_and_preserves_user_state() {
    let (root, input, scan) = fixture(true);
    write(root.path(), "sample.txt", "staged\n");
    git(root.path(), &["add", "sample.txt"]);
    write(root.path(), "sample.txt", "");
    write(root.path(), "alpha/a.py", "dirty\n");
    write(root.path(), "untracked", "keep me\n");
    let status = git(root.path(), &["status", "--porcelain=v1"]);
    let index = fs::read(root.path().join(".git/index")).unwrap();
    let docs = fs::read(root.path().join("docs/tower/user.md")).unwrap();
    let report = verify(&VerificationRequest {
        root: input.root.clone(),
        common_dir: input.common_dir.clone(),
        revision: input.revision.clone(),
        commands: verification_commands(&input, &scan).unwrap(),
        timeout: Duration::from_secs(3),
        log_root: input.common_dir.join("towermd/cache/verification"),
        cancelled: Arc::new(AtomicBool::new(false)),
    })
    .unwrap();
    if let Ok(evidence) = std::env::var("TOWERMD_COLDSTART_TEST_EVIDENCE") {
        let evidence = Path::new(&evidence);
        fs::create_dir_all(evidence).unwrap();
        let sealed = Path::new(&report.executions[0].stdout_log)
            .parent()
            .unwrap();
        fs::copy(
            sealed.join("report.json"),
            evidence.join("sealed-report.json"),
        )
        .unwrap();
        fs::copy(
            sealed.join("report.sha256"),
            evidence.join("sealed-report.sha256"),
        )
        .unwrap();
        fs::write(
            evidence.join("early-actual-report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        for (index, execution) in report.executions.iter().enumerate() {
            fs::copy(
                &execution.stdout_log,
                evidence.join(format!("early-{index}-stdout.log")),
            )
            .unwrap();
            fs::copy(
                &execution.stderr_log,
                evidence.join(format!("early-{index}-stderr.log")),
            )
            .unwrap();
        }
    }
    assert_eq!(report.executions.len(), 2);
    let sealed = Path::new(&report.executions[0].stdout_log)
        .parent()
        .unwrap();
    assert_eq!(
        digest(&fs::read(sealed.join("report.json")).unwrap()),
        report_hash(&report).unwrap()
    );
    for execution in &report.executions {
        assert_eq!(
            digest(&fs::read(&execution.stdout_log).unwrap()),
            execution.stdout_sha256
        );
        assert_eq!(
            digest(&fs::read(&execution.stderr_log).unwrap()),
            execution.stderr_sha256
        );
    }
    let results = aggregate_verification(&input, &scan, &report).unwrap();
    for root in ["alpha", "beta", "gamma"] {
        let result = results
            .iter()
            .find(|r| r.module == module(&scan, root)["id"])
            .unwrap();
        assert_eq!(result.result, EvidenceResult::Passed);
        assert_eq!(result.execution_indices, [0, 1]);
    }
    assert!(fs::read_to_string(&report.executions[0].stdout_log)
        .unwrap()
        .contains("coldstart-real-check-marker"));
    assert!(fs::read_to_string(&report.executions[1].stdout_log)
        .unwrap()
        .contains("coldstart-real-test-marker"));
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    assert_eq!(git(root.path(), &["status", "--porcelain=v1"]), status);
    assert_eq!(git(root.path(), &["rev-parse", "main"]), input.revision);
    assert_eq!(
        fs::read(root.path().join("docs/tower/user.md")).unwrap(),
        docs
    );
    assert_eq!(
        fs::read_to_string(root.path().join("sample.txt")).unwrap(),
        ""
    );
    assert_eq!(
        fs::read_to_string(root.path().join("alpha/a.py")).unwrap(),
        "dirty\n"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("untracked")).unwrap(),
        "keep me\n"
    );
    assert!(!root.path().join("aggregation-count").exists());
    assert!(!root.path().join("pending-marker").exists());
    assert_eq!(
        fs::read_dir(input.common_dir.join("towermd/verify"))
            .unwrap()
            .count(),
        0
    );
    if let Ok(evidence) = std::env::var("TOWERMD_COLDSTART_TEST_EVIDENCE") {
        let evidence = Path::new(&evidence);
        fs::create_dir_all(evidence).unwrap();
        fs::write(
            evidence.join("actual-report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        fs::write(
            evidence.join("module-results.json"),
            serde_json::to_vec_pretty(&results).unwrap(),
        )
        .unwrap();
        fs::write(
            evidence.join("scan-input.json"),
            serde_json::to_vec_pretty(&input).unwrap(),
        )
        .unwrap();
        fs::write(
            evidence.join("prepared-scan.json"),
            serde_json::to_vec_pretty(&scan).unwrap(),
        )
        .unwrap();
        for (index, execution) in report.executions.iter().enumerate() {
            fs::copy(
                &execution.stdout_log,
                evidence.join(format!("{index}-stdout.log")),
            )
            .unwrap();
            fs::copy(
                &execution.stderr_log,
                evidence.join(format!("{index}-stderr.log")),
            )
            .unwrap();
        }
        fs::write(evidence.join("protection.json"), serde_json::to_vec_pretty(&json!({"status_before":status,"status_after":git(root.path(), &["status", "--porcelain=v1"]),"index_before":digest(&index),"index_after":digest(&fs::read(root.path().join(".git/index")).unwrap()),"main_before":input.revision,"main_after":git(root.path(), &["rev-parse", "main"]),"docs_unchanged":fs::read(root.path().join("docs/tower/user.md")).unwrap()==docs})).unwrap()).unwrap();
    }
}

#[test]
fn an_own_failed_command_does_not_fail_modules_using_shared_root_commands() {
    let (root, _, _) = fixture(true);
    write(root.path(), "alpha/Makefile", "test:\n\t@false\n");
    git(root.path(), &["add", "alpha/Makefile"]);
    git(root.path(), &["commit", "-qm", "own test entry"]);
    let input = capture_scan_input(
        &Catalog::load(root.path()).unwrap(),
        &ScanOptions::default(),
    )
    .unwrap();
    let scan = prepare_scan(&input).unwrap();
    let mut report = report(&input, &scan);
    let own = report
        .executions
        .iter_mut()
        .find(|e| e.command.cwd == Path::new("alpha"))
        .unwrap();
    own.result = "failed".into();
    own.exit_code = Some(1);
    own.diagnostic = Some("command exit failure".into());
    let results = aggregate_verification(&input, &scan, &report).unwrap();
    let alpha = results
        .iter()
        .find(|r| r.module == module(&scan, "alpha")["id"])
        .unwrap();
    assert_eq!(alpha.result, EvidenceResult::Failed);
    assert!(!alpha.shared);
    assert_eq!(alpha.execution_indices.len(), 1);
    for root in ["beta", "gamma"] {
        let result = results
            .iter()
            .find(|r| r.module == module(&scan, root)["id"])
            .unwrap();
        assert_eq!(result.result, EvidenceResult::Passed);
        assert!(result.shared);
        assert_eq!(result.execution_indices.len(), 2);
    }
}

#[test]
fn failed_without_diagnostic_remains_failed_and_keeps_none() {
    let (_root, input, scan) = fixture(true);
    for exit_code in [None, Some(7), Some(0)] {
        let mut report = report(&input, &scan);
        report.executions[0].result = "failed".into();
        report.executions[0].exit_code = exit_code;
        report.executions[0].diagnostic = None;
        let results = aggregate_verification(&input, &scan, &report)
            .expect("failed does not require a diagnostic");
        let referencing = results
            .iter()
            .filter(|r| !r.execution_indices.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(referencing.len(), 3);
        for result in referencing {
            assert_eq!(result.result, EvidenceResult::Failed);
            assert!(result.diagnostics.is_empty());
            assert_eq!(result.report_hash, report_hash(&report).unwrap());
        }
        assert_eq!(report.executions[0].diagnostic, None);
    }
}

#[test]
fn passed_with_diagnostic_uses_exit_and_timeout_facts_and_retains_text() {
    let (_root, input, scan) = fixture(true);
    let mut report = report(&input, &scan);
    report.executions[0].diagnostic = Some("verifier note".into());
    let results = aggregate_verification(&input, &scan, &report)
        .expect("diagnostic does not invalidate successful execution facts");
    for result in results.iter().filter(|r| !r.execution_indices.is_empty()) {
        assert_eq!(result.result, EvidenceResult::Passed);
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d["reason"] == "verifier note" && d["execution_index"] == 0));
        assert_eq!(result.report_hash, report_hash(&report).unwrap());
    }
}
