#![cfg(unix)]
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
use towermd_core::{
    catalog::Catalog,
    cold_start::{apply_verification, scan, validate_verification, validate_verification_logs},
    config::{Config, CONFIG_PATH},
    okf::Document,
    verifier::{self, Limits, VerificationPlan, VerificationStatus},
};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Verification Test",
            "-c",
            "user.email=qa@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn actual_results_bind_candidate_objects_without_moving_nodes_and_reject_changed_logs() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "-qb", "main"]);
    fs::create_dir(root.join(".towermd")).unwrap();
    fs::write(
        root.join(CONFIG_PATH),
        Config::new("qa", "docs/tower").unwrap().to_yaml(),
    )
    .unwrap();
    for name in ["client", "engine", "storage"] {
        fs::create_dir(root.join(name)).unwrap();
        fs::write(root.join(name).join("main.rs"), "fn main() {}\n").unwrap();
    }
    fs::write(
        root.join("Makefile"),
        "test:\n\t@printf 'actual verifier output\\n'\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    let mut candidate = scan(
        &Catalog::load(root).unwrap(),
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let positions: Vec<_> = candidate.view.nodes.iter().map(|n| (n.x, n.y)).collect();
    let result = verifier::run(
        &VerificationPlan {
            root: root.to_path_buf(),
            id: candidate.id.clone(),
            revision: candidate.revision.clone(),
            tree: candidate.tree.clone(),
            commands: candidate.commands.clone(),
            modules: candidate.modules.clone(),
            limits: Limits::default(),
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(result.commands.len(), 1);
    assert_eq!(result.commands[0].status, VerificationStatus::Passed);
    assert!(result
        .modules
        .iter()
        .all(|m| m.shared && m.status == VerificationStatus::Passed));
    let mut forged = result.clone();
    forged.commands[0].exit_code = Some(7);
    let before = serde_json::to_vec(&candidate).unwrap();
    assert!(apply_verification(&mut candidate, forged).is_err());
    assert_eq!(serde_json::to_vec(&candidate).unwrap(), before);
    apply_verification(&mut candidate, result).unwrap();
    validate_verification(&candidate).unwrap();
    validate_verification_logs(&candidate, &root.join(".git")).unwrap();
    assert_eq!(
        positions,
        candidate
            .view
            .nodes
            .iter()
            .map(|n| (n.x, n.y))
            .collect::<Vec<_>>()
    );
    for node in &candidate.view.nodes {
        let object = Document::parse(&node.content).unwrap();
        assert_eq!(object.get_str("status"), Some("draft"));
        assert_eq!(object.get_str("verification"), Some("passed"));
        let evidence = object.get("initial_evidence").unwrap();
        assert_eq!(evidence["actor"].as_str(), Some("process:towermd-verifier"));
        assert_eq!(evidence["tree"].as_str(), Some(candidate.tree.as_str()));
        assert!(!object.body().contains("尚未执行"));
    }
    let mut wrong_tree = candidate.clone();
    wrong_tree.verification.as_mut().unwrap().tree = "0".repeat(40);
    assert!(validate_verification(&wrong_tree).is_err());
    let mut wrong_status = candidate.clone();
    wrong_status.verification.as_mut().unwrap().modules[0].status = VerificationStatus::Cancelled;
    assert!(validate_verification(&wrong_status).is_err());
    let log = candidate.verification.as_ref().unwrap().commands[0]
        .log
        .as_ref()
        .unwrap();
    fs::write(root.join(".git").join(log), "changed").unwrap();
    assert!(validate_verification_logs(&candidate, &root.join(".git")).is_err());
    assert!(!root
        .join(".git/towermd/verify")
        .join(&candidate.id)
        .join("worktree")
        .exists());
}
