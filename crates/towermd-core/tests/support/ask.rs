#![allow(dead_code)]
pub use super::baseline_fixture::*;
use std::sync::Arc;
use towermd_core::{
    baseline_review::review_baseline_shared,
    catalog::Catalog,
    human_intervention::HumanCommand,
    project_session::{ask::AskProgress, ProjectSession},
    recovery_contract::baseline::Verdict,
    snapshot_service::PublicationReport,
    tasks::{TaskCommand, TaskSpec},
};
pub fn claimed() -> Fixture {
    claimed_with_decision(false)
}
pub fn claimed_with_decision(decision: bool) -> Fixture {
    let mut files = vec![(
        "docs/tower/specs/AC-ask.md",
        "---\ntype: Acceptance\nid: AC-ask\nmodule: A-alpha\ncriteria: [actual bounded ask]\n---\n",
    )];
    if decision {
        files.push(("docs/tower/decisions/D-ask.md","---\ntype: Decision\nid: D-ask\nstatus: stable\ncontext: 权威问题？\ndecision: 使用仓库答案\naffects: [A-alpha]\n---\n"));
    }
    let mut f = fixture_with_source_files(&files);
    let approval = action(&f, Verdict::Approved);
    let approval = review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut f.outbox,
        &approval,
        "human:owner",
        "host",
        NOW + 2,
    )
    .unwrap();
    let published = f
        .writer
        .publish_baseline(
            &f.session,
            &f.catalog,
            &mut f.outbox,
            &approval.receipt.result_ref.approval_eid,
            "host",
            || Ok(NOW + 5),
        )
        .unwrap();
    assert!(matches!(published, PublicationReport::Recorded { .. }));
    git(f.root.path(), &["switch", "-qc", "t/T-ask", "main"]);
    f.catalog = Arc::new(Catalog::load(f.root.path()).unwrap());
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Add {
            task: TaskSpec {
                id: "T-ask".into(),
                title: "actual ask".into(),
                module: "A-alpha".into(),
                touches: vec![],
                allowed_paths: vec!["alpha/**".into()],
                depends_on: vec![],
                acceptance: vec!["AC-ask".into()],
                decisions: vec![],
            },
        },
        None,
        NOW + 10,
        &mut f.outbox,
    )
    .unwrap();
    claim(&mut f, NOW + 11);
    f
}
pub fn claim(f: &mut Fixture, now: u64) {
    ProjectSession::execute_shared(
        &f.session,
        &f.catalog,
        &f.outbox.read_inputs().unwrap(),
        "host",
        TaskCommand::Claim {
            id: "T-ask".into(),
            harness: None,
            harness_version: None,
        },
        None,
        now,
        &mut f.outbox,
    )
    .unwrap();
}
pub fn open(question: &str) -> HumanCommand {
    HumanCommand::AskOpen {
        question: question.into(),
        options: vec!["继续".into(), "停止".into()],
        recommended: Some(0),
        context: "原上下文\r\n保留".into(),
    }
}
pub fn call(f: &mut Fixture, id: &str, command: &HumanCommand, now: u64) -> AskProgress {
    ProjectSession::ask_shared(
        &f.session,
        &f.catalog,
        "host",
        id,
        command,
        None,
        &mut f.outbox,
        || Ok(now),
    )
    .unwrap()
}
pub fn events(f: &Fixture) -> Vec<towermd_core::hub_protocol::Event> {
    f.outbox.read_inputs().unwrap().confirmed().to_vec()
}

pub fn reopen(mut f: Fixture, now: u64) -> Fixture {
    // Release the sole old local Hub before opening its persistent storage.
    // Assignment evaluates its RHS first, so replacing the field directly
    // would keep the old lock live while opening the new Hub.
    drop(f.session);
    f.session = Arc::new(std::sync::Mutex::new(
        ProjectSession::open(
            &f.catalog.common_dir.join("towermd"),
            &f.catalog,
            &f.outbox.read_inputs().unwrap(),
            now,
        )
        .unwrap(),
    ));
    f
}
