//! The task card (spec 10.4): everything a delegate needs to do one task,
//! printed by `towermd task claim` and `task show` in v0.1 and written to the
//! run directory as `task.md` by the M3 runner. It is derived on every read
//! from the task, the objects on main and the person's rules and prompts in
//! force, so nothing here is stored.
use std::collections::BTreeMap;

use serde::Serialize;

use crate::task_model::Task;
use towermd_core::okf::Document;

/// Decision summaries are cut to this many characters.
const SUMMARY_CHARS: usize = 120;

pub const PROHIBITIONS: [&str; 3] = [
    "不得修改慢层对象（L0–L2）、`tower.canvas` 与快照；需要改时用 `towermd propose`。",
    "不得修改 `allowed_paths` 以外的文件。",
    "未通过 `towermd dev finish` 不得合并。",
];
pub const INTERACTION: [&str; 2] = [
    "需要人回答或评审时执行 `towermd ask`；不要猜。",
    "返回 `deferred` 时结束本回合，并说明在等什么；回答会在下一次调用时送达。",
];
pub const CHECKPOINT: &str = "每完成一个可验证的小步就提交一次检查点。";
pub const REPORT: &str = "收尾时用 `towermd dev finish --report <file>` 提交运行报告（FR-28）；报告只是自述，进度只认证据。";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CardInterface {
    pub id: String,
    pub title: String,
    pub from: String,
    pub to: String,
    pub contract: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CardDecision {
    pub id: String,
    pub title: String,
    /// `None` when the decision is not on main.
    pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TaskCard {
    pub id: String,
    pub title: String,
    pub module: String,
    pub touches: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub baseline: String,
    /// The card's done-when criteria.
    pub acceptance: Vec<String>,
    /// Criteria of the module's acceptance objects on main.
    pub module_acceptance: Vec<String>,
    /// The module's verification commands; empty means the builtin check.
    pub verify: Vec<String>,
    /// Interface contracts the task's modules take part in (read-only).
    pub interfaces: Vec<CardInterface>,
    pub decisions: Vec<CardDecision>,
    pub prohibitions: Vec<String>,
    pub interaction: Vec<String>,
    /// Temporary rules and the person's prompts in force, as delivered.
    pub rules: Vec<String>,
    pub prompts: Vec<String>,
    pub checkpoints: Vec<String>,
    pub report: String,
    /// The same card as Markdown, for the terminal and the run directory.
    pub markdown: String,
}

fn strings(document: &Document, key: &str) -> Vec<String> {
    document.get(key).and_then(serde_yaml::Value::as_sequence).into_iter().flatten()
        .filter_map(serde_yaml::Value::as_str).map(str::to_owned).collect()
}

fn title(document: &Document, id: &str) -> String {
    document.get_str("title").or_else(|| document.get_str("name")).filter(|title| !title.trim().is_empty()).unwrap_or(id).to_owned()
}

fn summary(document: &Document) -> Option<String> {
    let text = document.get_str("summary").map(str::to_owned).or_else(|| {
        document.body().lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with('#')).map(str::to_owned)
    })?;
    let mut chars = text.chars();
    let cut: String = chars.by_ref().take(SUMMARY_CHARS).collect();
    Some(if chars.next().is_some() { format!("{cut}…") } else { cut })
}

/// Builds the card. `objects` are the documents on main by ID; `rules` and
/// `prompts` are the texts in force for this task, oldest first.
pub fn build(task: &Task, objects: &BTreeMap<String, Document>, rules: Vec<String>, prompts: Vec<String>) -> TaskCard {
    let input = &task.input;
    let module = objects.get(&input.module).filter(|document| document.doc_type() == Some("Module"));
    let modules: Vec<&str> = std::iter::once(input.module.as_str()).chain(input.touches.iter().map(String::as_str)).collect();
    let mut interfaces = Vec::new();
    let mut module_acceptance = Vec::new();
    for (id, document) in objects {
        match document.doc_type() {
            Some("Interface") => {
                let (Some(from), Some(to)) = (document.get_str("from"), document.get_str("to")) else { continue };
                if modules.contains(&from) || modules.contains(&to) {
                    interfaces.push(CardInterface { id: id.clone(), title: title(document, id), from: from.into(), to: to.into(),
                        contract: document.get_str("contract").unwrap_or_default().to_owned() });
                }
            }
            Some("Acceptance") if document.get_str("module") == Some(input.module.as_str()) => {
                module_acceptance.extend(strings(document, "criteria"));
            }
            _ => {}
        }
    }
    let decisions = input.decisions.iter().map(|id| match objects.get(id) {
        Some(document) => CardDecision { id: id.clone(), title: title(document, id), summary: summary(document) },
        None => CardDecision { id: id.clone(), title: id.clone(), summary: None },
    }).collect();
    let mut checkpoints = vec![CHECKPOINT.to_owned()];
    checkpoints.extend(input.checkpoints.iter().map(|checkpoint| match &checkpoint.due {
        Some(due) => format!("{}（{due} 前）", checkpoint.text),
        None => checkpoint.text.clone(),
    }));
    let mut card = TaskCard {
        id: input.id.clone(),
        title: input.title.clone(),
        module: input.module.clone(),
        touches: input.touches.clone(),
        allowed_paths: input.allowed_paths.clone(),
        baseline: task.baseline.clone(),
        acceptance: input.acceptance.clone(),
        module_acceptance,
        verify: module.map(|module| strings(module, "verify")).unwrap_or_default(),
        interfaces,
        decisions,
        prohibitions: PROHIBITIONS.iter().map(|text| (*text).to_owned()).collect(),
        interaction: INTERACTION.iter().map(|text| (*text).to_owned()).collect(),
        rules,
        prompts,
        checkpoints,
        report: REPORT.into(),
        markdown: String::new(),
    };
    card.markdown = markdown(&card);
    card
}

fn section(text: &mut String, heading: &str, lines: &[String], empty: &str) {
    text.push_str(&format!("\n## {heading}\n\n"));
    if lines.is_empty() {
        text.push_str(&format!("{empty}\n"));
    }
    for line in lines {
        text.push_str(&format!("- {line}\n"));
    }
}

fn markdown(card: &TaskCard) -> String {
    let code = |items: &[String]| items.iter().map(|item| format!("`{item}`")).collect::<Vec<_>>().join("、");
    let mut text = format!("# 任务卡 {}：{}\n\n", card.id, card.title);
    text.push_str(&format!("- 所属模块：{}\n", card.module));
    if !card.touches.is_empty() {
        text.push_str(&format!("- 同时触及：{}\n", card.touches.join("、")));
    }
    text.push_str(&format!("- 允许修改的路径：{}\n- 基线：{}\n", code(&card.allowed_paths), card.baseline));
    let mut acceptance = card.acceptance.clone();
    acceptance.extend(card.module_acceptance.iter().map(|criterion| format!("（模块验收）{criterion}")));
    section(&mut text, "验收标准", &acceptance, "未写完成标准。");
    let verify: Vec<String> = card.verify.iter().map(|command| format!("`{command}`")).collect();
    section(&mut text, "验证命令", &verify, "模块未配验证命令；`towermd dev finish` 由内置检查 `core.verify` 兜底。");
    let interfaces: Vec<String> = card.interfaces.iter().map(|interface| format!("{}（{} → {}）：{}", interface.title, interface.from, interface.to, interface.contract)).collect();
    section(&mut text, "相关接口契约（只读）", &interfaces, "无。");
    let decisions: Vec<String> = card.decisions.iter().map(|decision| match &decision.summary {
        Some(summary) => format!("{} {}：{summary}", decision.id, decision.title),
        None => format!("{}（main 上未找到）", decision.id),
    }).collect();
    section(&mut text, "相关决策", &decisions, "无。");
    section(&mut text, "禁止事项", &card.prohibitions, "");
    section(&mut text, "交互约定", &card.interaction, "");
    let mut rules = card.rules.clone();
    rules.extend(card.prompts.iter().map(|prompt| format!("人工插话：{prompt}")));
    section(&mut text, "当前生效的临时规则与插话", &rules, "无。");
    section(&mut text, "检查点", &card.checkpoints, "");
    text.push_str(&format!("\n## 报告\n\n{}\n", card.report));
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_model::{TaskDraft, TaskOrigin, TaskState};
    use towermd_core::delegation::Checkpoint;

    fn doc(text: &str) -> Document {
        Document::parse(text).unwrap()
    }

    fn task() -> Task {
        Task {
            input: TaskDraft { id: "T-1".into(), title: "同步队列".into(), module: "sync".into(), touches: vec!["store".into()],
                allowed_paths: vec!["src/sync/**".into()], acceptance: vec!["离线编辑能补发".into()], decisions: vec!["D-1".into(), "D-9".into()],
                checkpoints: vec![Checkpoint { text: "队列落盘".into(), due: None }], ..TaskDraft::default() },
            origin: TaskOrigin::Cli, baseline: "towermd/B0".into(), state: TaskState::Ready, needs_review: false, active_run: None,
            gate_failures: 0, interrupt: None,
        }
    }

    fn objects() -> BTreeMap<String, Document> {
        BTreeMap::from([
            ("sync".into(), doc("---\ntype: Module\nid: sync\ntitle: 同步\npaths: [src/sync/**]\nverify: [cargo test -p sync]\n---\n")),
            ("I-store".into(), doc("---\ntype: Interface\nid: I-store\ntitle: 存储接口\nfrom: store\nto: ui\ncontract: put/get 幂等\n---\n")),
            ("I-other".into(), doc("---\ntype: Interface\nid: I-other\nfrom: ui\nto: net\ncontract: x\n---\n")),
            ("AC-sync".into(), doc("---\ntype: Acceptance\nid: AC-sync\nmodule: sync\ncriteria: [断网 10 分钟后不丢数据]\n---\n")),
            ("D-1".into(), doc("---\ntype: Decision\nid: D-1\ntitle: 用 SQLite\n---\n# 标题\n\n本地队列放在 SQLite 里，不用文件。\n")),
        ])
    }

    #[test]
    fn card_carries_every_section_of_spec_10_4() {
        let card = build(&task(), &objects(), vec!["临时规则（2026-10-25 前有效）：先不动 UI".into()], vec!["先写测试".into()]);
        assert_eq!(card.verify, ["cargo test -p sync"]);
        assert_eq!(card.module_acceptance, ["断网 10 分钟后不丢数据"]);
        assert_eq!(card.interfaces.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["I-store"], "只列任务模块参与的接口");
        assert_eq!(card.decisions[0].summary.as_deref(), Some("本地队列放在 SQLite 里，不用文件。"));
        assert_eq!(card.decisions[1].summary, None);
        assert_eq!(card.checkpoints, [CHECKPOINT, "队列落盘"]);
        for needle in ["# 任务卡 T-1：同步队列", "`src/sync/**`", "`cargo test -p sync`", "存储接口（store → ui）：put/get 幂等",
            "D-9（main 上未找到）", "不得修改 `allowed_paths` 以外的文件", "`towermd ask`", "`deferred`", "人工插话：先写测试",
            "先不动 UI", "提交一次检查点", "`towermd dev finish --report <file>`", "（模块验收）断网 10 分钟后不丢数据"] {
            assert!(card.markdown.contains(needle), "{needle}\n{}", card.markdown);
        }
    }

    #[test]
    fn a_module_without_commands_names_the_builtin_check() {
        let mut objects = objects();
        objects.insert("sync".into(), doc("---\ntype: Module\nid: sync\npaths: [src/sync/**]\n---\n"));
        let card = build(&task(), &objects, vec![], vec![]);
        assert!(card.verify.is_empty() && card.markdown.contains("core.verify"));
    }
}
