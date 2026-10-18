//! Ask requests remain process-authored. Answers here are explicitly relayed,
//! never human approvals or durable Decision/Rule publication.
use super::*;
use serde_json::Value;
use towermd_core::{human_intervention::HumanCommand, tasks::SelfReported};

#[derive(clap::Args)]
#[command(group(clap::ArgGroup::new("ask_mode").args(["question", "wait", "reply"]).required(true).multiple(false)))]
pub struct AskArgs {
    #[arg(help = "提问原文；默认最多等待90秒")]
    question: Option<String>,
    #[arg(long, requires = "question", conflicts_with_all = ["wait", "reply"], help = "可选答案，可重复")]
    option: Vec<String>,
    #[arg(long, requires = "question", conflicts_with_all = ["wait", "reply"], help = "推荐答案的索引，从0开始")]
    recommended: Option<usize>,
    #[arg(long, requires = "question", conflicts_with_all = ["wait", "reply"])]
    context: Option<String>,
    #[arg(long, help = "继续等待已有 Ask Hold，最多90秒")]
    wait: Option<String>,
    #[arg(long, requires = "answer", help = "转述人的回答；不生成批准或仓库规则")]
    reply: Option<String>,
    #[arg(long, requires = "reply", conflicts_with_all = ["question", "wait"])]
    answer: Option<String>,
    #[arg(long, requires_all = ["reply", "harness_version"], conflicts_with_all = ["question", "wait"])]
    harness: Option<String>,
    #[arg(long, requires_all = ["reply", "harness"], conflicts_with_all = ["question", "wait"])]
    harness_version: Option<String>,
    #[arg(long, help = "不确定重试时沿用原请求 UUID 与原输入")]
    request_id: Option<String>,
}

fn request(
    directory: PathBuf,
    run: Option<String>,
    id: String,
    command: HumanCommand,
) -> Result<Value, String> {
    towermd_core::recovery_contract::recorded::request_event_eid(&id, "hold.opened")?;
    daemon::request_interaction(directory, run, id.clone(), command).map_err(|error| {
        format!("request_id={id}：{error}；请保留原输入，用 --request-id {id} 重试")
    })
}

pub fn run(args: AskArgs, run: Option<String>) -> Result<(), String> {
    let directory = std::env::current_dir().map_err(|e| e.to_string())?;
    let id = args
        .request_id
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let command = if let Some(hold) = args.wait {
        HumanCommand::AskWait { hold }
    } else if let Some(hold) = args.reply {
        HumanCommand::Relay {
            hold,
            text: args.answer.ok_or("--reply 需要 --answer")?,
            self_reported: args
                .harness
                .zip(args.harness_version)
                .map(|(harness, version)| SelfReported { harness, version }),
        }
    } else {
        let question = args.question.ok_or("缺少问题原文")?;
        if towermd_core::human_intervention::normalize_question(&question).is_empty() {
            return Err("问题不能仅含空白或结尾标点".into());
        }
        if args
            .recommended
            .is_some_and(|index| index >= args.option.len())
        {
            return Err("--recommended 超出 --option 范围（索引从0开始）".into());
        }
        HumanCommand::AskOpen {
            question,
            options: args.option,
            recommended: args.recommended,
            context: args.context.unwrap_or_default(),
        }
    };
    let is_open = matches!(command, HumanCommand::AskOpen { .. });
    let result = request(directory.clone(), run.clone(), id.clone(), command)?;
    let result = if is_open && result["current"]["state"] == "pending" {
        let hold = result["hold"]
            .as_str()
            .ok_or("Ask 返回缺少 Hold ID")?
            .to_owned();
        let wait_id = uuid::Uuid::now_v7().to_string();
        let resume_run = run
            .as_ref()
            .map(|id| format!(" --run {id}"))
            .unwrap_or_default();
        request(directory, run, wait_id.clone(), HumanCommand::AskWait { hold: hold.clone() })
            .map_err(|error| format!("Ask 已创建 hold={hold}，open_request_id={id}；{error}；继续原等待：towermd ask --wait {hold} --request-id {wait_id}{resume_run}"))?
    } else {
        result
    };
    print_json(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn accepts(args: &[&str]) -> bool {
        let mut command = vec!["towermd", "ask"];
        command.extend_from_slice(args);
        Cli::try_parse_from(command).is_ok()
    }
    #[test]
    fn finite_ask_modes_and_relay_identity_are_explicit() {
        for args in [
            vec!["问题"],
            vec![
                "问题",
                "--option",
                "是",
                "--option",
                "否",
                "--recommended",
                "0",
            ],
            vec!["--wait", "H-1"],
            vec!["--reply", "H-1", "--answer", "先停止"],
            vec![
                "--reply",
                "H-1",
                "--answer",
                "原文",
                "--harness",
                "codex",
                "--harness-version",
                "1",
            ],
        ] {
            assert!(accepts(&args), "{args:?}");
        }
        for args in [
            vec![],
            vec!["问题", "--wait", "H-1"],
            vec!["--reply", "H-1"],
            vec!["--answer", "答复"],
            vec!["--wait", "H-1", "--option", "是"],
            vec!["--wait", "H-1", "--recommended", "0"],
            vec!["--wait", "H-1", "--context", "context"],
            vec!["--reply", "H-1", "--answer", "答复", "--option", "是"],
            vec!["问题", "--answer", "答复"],
            vec!["--wait", "H-1", "--answer", "答复"],
            vec!["问题", "--harness", "codex", "--harness-version", "1"],
            vec![
                "--wait",
                "H-1",
                "--harness",
                "codex",
                "--harness-version",
                "1",
            ],
            vec!["--reply", "H-1", "--answer", "答复", "--harness", "codex"],
            vec!["问题", "--actor", "human:owner"],
            vec!["问题", "--approve"],
            vec!["问题", "--save-decision"],
            vec!["问题", "--recommended", "-1"],
        ] {
            assert!(!accepts(&args), "{args:?}");
        }
    }
}
