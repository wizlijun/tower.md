//! Spec 9.1 "一句话生长" on a code repository: the seed sentence as the
//! draft intent I-1 and the skeleton's limits. Nothing here writes: the
//! delegate turns the plan into a skeleton commit and an S6 submission, and
//! only a person's approval in the App makes it B0.
use serde::Serialize;
use towermd_core::{ideas, okf::Document, types};

pub const INTENT: &str = "I-1";
pub const MAX_QUESTIONS: usize = 3;
pub const MAX_MODULES: usize = 3;
pub const MAX_TASKS: usize = 7;
pub const FIRST_TASK: &str = "空骨架能构建并通过一条冒烟测试";

/// Where the sentence came from: typed now, or the goal a person promoted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Argument,
    Goal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Intent {
    pub id: &'static str,
    /// Relative to the S6 submission, as `dev draft submit` expects.
    pub path: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Limits {
    pub questions: usize,
    pub modules: [usize; 2],
    pub tasks: usize,
    pub first_task: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Seed {
    pub sentence: String,
    pub origin: Origin,
    /// The promoted idea, when the sentence is the project's goal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idea: Option<String>,
    pub intent: Intent,
    pub limits: Limits,
    pub steps: Vec<&'static str>,
}

const STEPS: [&str; 7] = [
    "澄清：最多 3 个问题。在会话中直接问人；需要人在审阅时看到的，写进 S6 提交的 questions。",
    "骨架：按 1–3 个模块写最小可构建的代码和一条冒烟测试，作为普通提交合入 main。",
    "towermd dev scan start，再 towermd dev scan wait <扫描 ID>，得到候选模块和验证命令。",
    "towermd dev draft template <扫描 ID> 取 S6 模板；把 intent 原样放进 intents，allocated_to 填对应模块，为每个模块写验收标准。",
    "towermd dev draft submit <文件> --producer <harness>/<版本> 提交草稿基线。",
    "等人在 App 审阅、验证并批准 B0；Agent 不能批准。",
    "批准后 towermd task add 至多 7 个任务，第一个是“空骨架能构建并通过一条冒烟测试”。",
];

/// The seed plan for `sentence`, or for the promoted goal when no sentence
/// is given.
pub fn plan(sentence: Option<&str>, ideas: &ideas::Ideas) -> Result<Seed, String> {
    let (sentence, origin, idea) = match sentence {
        Some(sentence) => (sentence.trim().to_owned(), Origin::Argument, None),
        None => {
            let goal = ideas::goal(ideas).ok_or("没有给出一句话，项目也还没有立项的目标；请写下这句话，或先在 App 中把想法立项为项目目标")?;
            (goal.text.clone(), Origin::Goal, Some(goal.id.clone()))
        }
    };
    ideas::check_text(&sentence)?;
    let layer = types::lookup("Intent").ok_or("意图类型未登记")?.default_layer.as_str();
    let mut document = Document::parse(&format!("---\ntype: Intent\n---\n\n{sentence}\n"))?;
    document.set("id", INTENT.into())?;
    document.set("layer", layer.into())?;
    document.set("status", "draft".into())?;
    document.set("title", sentence.as_str().into())?;
    document.set("allocated_to", serde_yaml::Value::Sequence(Vec::new()))?;
    Ok(Seed {
        sentence, origin, idea,
        intent: Intent { id: INTENT, path: format!("intents/{INTENT}.md"), text: document.to_text() },
        limits: Limits { questions: MAX_QUESTIONS, modules: [1, MAX_MODULES], tasks: MAX_TASKS, first_task: FIRST_TASK },
        steps: STEPS.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sentence_becomes_a_draft_l0_intent() {
        let seed = plan(Some("  做一个把会议录音转成待办的命令行工具 "), &ideas::Ideas::new()).unwrap();
        assert_eq!((seed.sentence.as_str(), seed.origin, seed.intent.path.as_str()), ("做一个把会议录音转成待办的命令行工具", Origin::Argument, "intents/I-1.md"));
        let document = Document::parse(&seed.intent.text).unwrap();
        assert_eq!((document.doc_type(), document.get_str("id"), document.get_str("layer"), document.get_str("status")),
            (Some("Intent"), Some("I-1"), Some("L0"), Some("draft")));
        assert_eq!(document.body().trim(), "做一个把会议录音转成待办的命令行工具");
        assert!(document.get("allocated_to").and_then(serde_yaml::Value::as_sequence).is_some_and(Vec::is_empty));
        assert_eq!((seed.limits.questions, seed.limits.modules, seed.limits.tasks), (3, [1, 3], 7));
    }

    #[test]
    fn without_a_sentence_only_the_promoted_goal_seeds() {
        assert!(plan(None, &ideas::Ideas::new()).unwrap_err().contains("立项"));
        assert!(plan(Some("两行\n文字"), &ideas::Ideas::new()).is_err());
        let ideas: ideas::Ideas = serde_json::from_value(serde_json::json!({
            "i-1": {"id":"i-1","text":"只是想法","added":{"at":"t","eid":"i-1","seq":1,"actor":"human:local"}},
            "i-2": {"id":"i-2","text":"做一个记账工具","added":{"at":"t","eid":"i-2","seq":2,"actor":"human:local"},
                "promoted":{"as":"project","event":{"at":"t","eid":"p","seq":3,"actor":"human:local"}}}
        })).unwrap();
        let seed = plan(None, &ideas).unwrap();
        assert_eq!((seed.sentence.as_str(), seed.origin, seed.idea.as_deref()), ("做一个记账工具", Origin::Goal, Some("i-2")));
    }
}
