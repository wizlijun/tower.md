//! CLI can request a Task review. Human verdicts only enter through the App.
use super::*;
use towermd_core::human_intervention::{HumanCommand, ReviewGate};

#[derive(Subcommand)]
pub enum ReviewArgs {
    #[command(about = "提交计划或已提交变更的评审请求，等待 App 明确放行")]
    Request {
        #[arg(long, value_enum)]
        gate: Gate,
        #[arg(long, help = "G1 计划原文文件（UTF-8）；G4 不接受此参数")]
        content: Option<PathBuf>,
        #[arg(long, help = "不确定重试时沿用原请求 UUID 与原输入")]
        request_id: Option<String>,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Gate {
    #[value(name = "G1")]
    G1,
    #[value(name = "G4")]
    G4,
}

pub fn run(arguments: ReviewArgs, run: Option<String>) -> Result<(), String> {
    let ReviewArgs::Request {
        gate,
        content,
        request_id,
    } = arguments;
    let directory = std::env::current_dir().map_err(|error| error.to_string())?;
    let (gate, content, source_path) = match (gate, content) {
        (Gate::G1, Some(path)) => {
            let source = path.to_str().ok_or("计划文件路径不是 UTF-8")?.to_owned();
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("无法读取 UTF-8 计划原文 {}：{error}", path.display()))?;
            (ReviewGate::G1, Some(text), Some(source))
        }
        (Gate::G1, None) => return Err("G1 评审需要 --content <UTF-8 文件>".into()),
        (Gate::G4, None) => (ReviewGate::G4, None, None),
        (Gate::G4, Some(_)) => return Err("G4 评审读取真实提交树，不接受 --content".into()),
    };
    let request_id = request_id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    towermd_core::recovery_contract::recorded::request_event_eid(&request_id, "hold.opened")?;
    let command = HumanCommand::ReviewRequest {
        gate,
        content,
        // The plan need not be committed or within Task.allowed_paths.
        source_commit: None,
        source_path,
    };
    let result = daemon::request_task_review(directory, run, request_id.clone(), command).map_err(
        |error| {
            format!(
                "request_id={request_id}：{error}；重试须使用 --request-id {request_id} 和原输入"
            )
        },
    )?;
    print_json(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_can_request_review_but_cannot_supply_verdict_or_actor() {
        assert!(Cli::try_parse_from([
            "towermd",
            "review",
            "request",
            "--gate",
            "G1",
            "--content",
            "plan.md",
        ])
        .is_ok());
        for suffix in [
            vec!["review", "approve", "H"],
            vec![
                "review",
                "request",
                "--gate",
                "G4",
                "--actor",
                "human:owner",
            ],
            vec!["review", "request", "--gate", "G4", "--verdict", "approved"],
            vec!["review", "request", "--gate", "G4", "--skip-review"],
        ] {
            let mut args = vec!["towermd"];
            args.extend(suffix);
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn invalid_content_combinations_fail_before_daemon_contact() {
        let missing = run(
            ReviewArgs::Request {
                gate: Gate::G1,
                content: None,
                request_id: None,
            },
            None,
        )
        .unwrap_err();
        assert!(missing.contains("G1") && missing.contains("--content"));
        let unwanted = run(
            ReviewArgs::Request {
                gate: Gate::G4,
                content: Some(PathBuf::from("does-not-exist")),
                request_id: None,
            },
            None,
        )
        .unwrap_err();
        assert!(unwanted.contains("G4") && unwanted.contains("不接受"));
    }
}
