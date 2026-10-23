//! FR-80 closed-task summaries for Note.md. A task is closed when main
//! admitted its passed evidence (`status: merged`); the summary cites the
//! admission commit and the task ID, nothing the delegate said about itself.
use serde_json::Value;
use towermd_core::notemd::{commit_id, commit_source, object_source, short, Capture};

/// One summary per merged evidence entry; a task reopened and merged again
/// gets a second summary under its new admission commit.
pub fn task_summaries(state: &Value, project_id: &str) -> Vec<Capture> {
    state["Evidence"].as_object().into_iter().flatten()
        .filter(|(_, entry)| entry["status"] == "merged")
        .filter_map(|(_, entry)| task_summary(state, entry, project_id))
        .collect()
}

fn task_summary(state: &Value, entry: &Value, project_id: &str) -> Option<Capture> {
    let task_id = entry["merge"]["task_id"].as_str()?;
    let commit = entry["merge"]["admission_commit"].as_str().filter(|commit| commit_id(commit))?;
    // The frozen finish input is what was verified; fall back to the card.
    let frozen = &entry["input"]["task"]["input"];
    let input = if frozen.is_object() { frozen } else { &state["Task"][task_id]["input"] };
    let title = input["title"].as_str().map(str::trim).filter(|title| !title.is_empty()).unwrap_or(task_id);
    let mut body = format!("# {task_id} {title}\n\n");
    body.push_str(&format!(
        "在 Tower.md 中关闭（任务 {task_id}[^task]）：完成证据通过验证并接入 main，提交 `{}`。[^commit]\n",
        short(commit)
    ));
    if let Some(why) = input["why"].as_str().map(str::trim).filter(|why| !why.is_empty()) {
        body.push_str(&format!("\n## 为什么\n\n{why}\n"));
    }
    let acceptance: Vec<_> = input["acceptance"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).map(str::trim).filter(|item| !item.is_empty()).collect();
    if !acceptance.is_empty() {
        body.push_str("\n## 验收\n\n");
        for item in acceptance {
            body.push_str(&format!("- {item}\n"));
        }
    }
    Some(Capture {
        name: format!("task-{task_id}-{}", short(commit)),
        doc_type: "Task Summary",
        title: format!("{task_id} {title}"),
        description: format!("Tower.md 任务 {task_id} 已关闭：{title}"),
        sources: vec![commit_source(commit), object_source("task", project_id, task_id, &format!("Tower.md 任务 {task_id}"))],
        notes: vec![],
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use towermd_core::okf::Document;

    const SHA: &str = "89abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn only_merged_evidence_becomes_a_summary() {
        let state = json!({
            "Task": {"T-9": {"input": {"id": "T-9", "title": "card title"}}},
            "Evidence": {
                "e1": {"status": "merged", "merge": {"task_id": "T-9", "admission_commit": SHA},
                    "input": {"task": {"input": {"id": "T-9", "title": "接入登录", "why": "用户要登录",
                        "acceptance": ["能登录", " "]}}}},
                "e2": {"status": "passed", "merge": {"task_id": "T-9", "admission_commit": SHA}},
                "e3": {"status": "merged", "merge": {"task_id": "T-9", "admission_commit": "bad"}},
                "e4": {"status": "merged", "merge": {"task_id": "T-9", "admission_commit": SHA.replace('8', "7")}}
            }
        });
        let summaries = task_summaries(&state, "p1");
        assert_eq!(summaries.len(), 2);
        let first = &summaries[0];
        assert_eq!(first.name, "task-T-9-89abcde");
        assert_eq!(summaries[1].title, "T-9 card title");
        let document = Document::parse(&first.render("2026-10-23T00:00:00Z").unwrap()).unwrap();
        assert_eq!(document.doc_type(), Some("Task Summary"));
        assert_eq!(document.get_str("title"), Some("T-9 接入登录"));
        let sources = document.get("sources").unwrap().as_sequence().unwrap();
        assert_eq!(sources[0].get("resource").unwrap().as_str(), Some(format!("git:{SHA}").as_str()));
        assert_eq!(sources[1].get("resource").unwrap().as_str(), Some("towermd:p1/T-9"));
        assert!(document.body().contains("## 为什么\n\n用户要登录\n") && document.body().contains("- 能登录\n"));
        assert!(!document.body().contains("- \n"));
    }
}
