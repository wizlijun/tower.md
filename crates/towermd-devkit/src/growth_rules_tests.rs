use serde_json::json;

use super::*;
use crate::review::ReviewedCommand;

fn module(id: &str, commands: usize) -> ReviewedModule {
    ReviewedModule {
        id: id.into(), title: id.into(), files: vec![format!("{id}/lib.rs")],
        commands: (0..commands).map(|index| ReviewedCommand { directory: String::new(), command: format!("test {index}") }).collect(),
        verification_pending: false, commands_confirmed: true,
    }
}

fn report(commands: &[&str], modules: &[(&str, &str)]) -> VerificationReport {
    serde_json::from_value(json!({
        "actor": "process:towermd", "revision": "r", "tree": "t", "started_at": "s", "finished_at": "f",
        "commands": commands.iter().enumerate().map(|(index, status)| json!({
            "id": format!("c{index}"), "directory": "", "argv": ["test"], "modules": [], "status": status,
            "exit_code": null, "duration_ms": 1, "log": null, "log_sha256": null, "log_excerpt": "",
            "log_truncated": false, "error": null,
        })).collect::<Vec<_>>(),
        "modules": modules.iter().map(|(id, status)| json!({"module": id, "status": status, "command_ids": [], "shared": false})).collect::<Vec<_>>(),
        "cleanup_complete": true,
    })).unwrap()
}

fn interface(id: &str, contract: &str) -> CanvasNode {
    serde_json::from_value(json!({
        "id": format!("node-{id}"), "kind": "text", "title": id, "x": 0, "y": 0, "width": 1, "height": 1,
        "object_id": id, "object_type": "Interface",
        "content": format!("---\ntype: Interface\nid: {id}\ncontract: {}\n---\n", serde_json::to_string(contract).unwrap()),
    })).unwrap()
}

#[test]
fn growing_b0_may_start_untested_but_every_later_step_needs_a_passing_smoke_test() {
    let untested = report(&[], &[("M-a", "no_tests")]);
    assert!(verification(Mode::Growing, true, &[module("M-a", 0)], &untested).is_ok());
    let error = verification(Mode::Growing, false, &[module("M-a", 0)], &untested).unwrap_err();
    assert!(error.contains("冒烟测试"), "{error}");
    let smoke = report(&["passed"], &[("M-a", "passed"), ("M-b", "no_tests")]);
    assert!(verification(Mode::Growing, false, &[module("M-a", 1), module("M-b", 0)], &smoke).is_ok());
}

#[test]
fn settled_needs_every_module_verified_by_its_own_commands_even_for_b0() {
    let partial = report(&["passed"], &[("M-a", "passed"), ("M-b", "no_tests")]);
    for first in [true, false] {
        let error = verification(Mode::Settled, first, &[module("M-a", 1), module("M-b", 0)], &partial).unwrap_err();
        assert!(error.contains("M-b") && !error.contains("M-a"), "{error}");
    }
    let full = report(&["passed", "passed"], &[("M-a", "passed"), ("M-b", "passed")]);
    assert!(verification(Mode::Settled, false, &[module("M-a", 1), module("M-b", 1)], &full).is_ok());
}

#[test]
fn settled_interfaces_must_name_an_existing_contract_file_or_test() {
    let nodes = [interface("IF-a", "调用方传入订单号"), interface("IF-b", "见 `api/tests/contract.rs`。")];
    let exists = |path: &str| path == "api/tests/contract.rs";
    assert!(interface_contracts(Mode::Growing, &nodes, exists).is_ok());
    let error = interface_contracts(Mode::Settled, &nodes, exists).unwrap_err();
    assert!(error.contains("IF-a") && !error.contains("IF-b"), "{error}");
    let escaping = [interface("IF-c", "../api/tests/contract.rs /etc/passwd")];
    assert!(interface_contracts(Mode::Settled, &escaping, |_| true).is_err());
    assert!(interface_contracts(Mode::Settled, &nodes[1..], exists).is_ok());
}

#[test]
fn impact_notes_explain_only_derived_objects_and_are_complete_once_settled() {
    let impact = Impact {
        intents: vec!["I-1".into()], modules: vec!["M-a".into(), "M-b".into()], interfaces: vec![],
        tasks: vec!["T-1".into()], paths: vec!["a/**".into()],
    };
    assert!(impact_notes(Mode::Growing, &impact, &BTreeMap::new()).is_ok());
    let partial = BTreeMap::from([("M-a".to_string(), "职责并入".to_string())]);
    assert!(impact_notes(Mode::Growing, &impact, &partial).is_ok());
    let error = impact_notes(Mode::Settled, &impact, &partial).unwrap_err();
    assert!(error.contains("I-1、M-b、T-1"), "{error}");
    let unknown = BTreeMap::from([("M-z".to_string(), "无关".to_string())]);
    assert!(impact_notes(Mode::Growing, &impact, &unknown).unwrap_err().contains("M-z"));
    let blank = BTreeMap::from([("M-a".to_string(), " ".to_string())]);
    assert!(impact_notes(Mode::Growing, &impact, &blank).is_err());
    let complete: BTreeMap<String, String> = ["I-1", "M-a", "M-b", "T-1"].iter().map(|id| (id.to_string(), format!("{id} 受影响"))).collect();
    assert!(impact_notes(Mode::Settled, &impact, &complete).is_ok());
}

#[test]
fn the_mode_follows_main_config_and_the_approved_module_count() {
    let config = |text: &str| Config::parse(&format!("docs_dir: docs\n{text}")).unwrap();
    assert_eq!(mode(&config("project_id: p\n"), 50), Mode::Growing);
    assert_eq!(mode(&config("project_id: p\ngrowth:\n  settle_at_modules: 3\n"), 3), Mode::Settled);
    assert_eq!(mode(&config("project_id: p\ngrowth:\n  mode: settled\n"), 0), Mode::Settled);
}
