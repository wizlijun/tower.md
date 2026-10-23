use std::process::Command;

use towermd_core::init::{init, render_agents_md, InitOptions};
use towermd_devkit::dev_state::KITS;

#[test]
fn dev_agents_md_matches_v0_1_1_except_dev_command_names() {
    // C0 moved finish and trailers under `towermd dev` (FR-200), and trailers
    // are read in text mode, where stdout holds only the trailer lines; the fixture
    // stays the v0.1.1 original so any other drift still fails here.
    // Delegation (FR-184, FR-201, FR-203, FR-209, FR-211) adds the card,
    // clarify, fingerprint and reply rules, and the self-check and claims;
    // the Tower API (FR-114–118) adds report and signal; node questions
    // (FR-127) add `--ref` and `--answers`; gates (FR-122) add what a
    // failed declared check means.
    let delegation = "- 领取前用 `towermd task show <task-id>` 读委托卡（目的、完成标准、边界、检查点、交付前自检）、这类工作的规则和上次退回的理由，并照办。\n\
        - 任务处于先澄清（`clarifying`）时不领取：用 `towermd dev clarify <task-id> <file>` 交上复述、完成标准草稿和最多 3 个问题，然后停下，等人在 App 中定稿。\n\
        - 领取时如实传自报配置：`towermd task claim <task-id> --harness <name> --harness-version <version>`，能核实时再加 `--vendor`、`--model`、`--skills-version`；不能编造。\n\
        - `delivered[]` 中的 `request_report` 是汇报请求，用 `towermd reply <request-id> --state in_progress --summary <摘要>` 回应；`--state` 按实情取 `not_started`、`in_progress`、`waiting`、`blocked` 或 `claimed_done`，摘要不超过 100 字。汇报不算进度。人在节点上的提问也以 `request_report` 送达，文字里带 `--ref <依据>`：回答时用 `--ref` 写明依据（文件、提交、文档），答不了就如实说明，不要猜。只有确实能回答某个节点的问题时，才用 `towermd report --target <节点> --answers` 声明，不再能答时用 `--no-answers` 撤回。\n\
        - 工作中可随时用 `towermd report --state in_progress --summary <摘要> --progress 3/7` 主动汇报进度，用 `towermd signal <kind> --level passive --title <标题>` 上报值得人注意的事；种类带命名空间（如 `eng.crap`），标题不超过 30 字，只有确实需要人看时才用 `active`。运行中二者默认指向当前任务。汇报和上报都不算进度。\n\
        - 只改任务卡";
    let expected = include_str!("fixtures/agents-v0.1.1.md")
        .replace("{docs_dir}", "docs/tower")
        .replace("towermd finish", "towermd dev finish")
        .replace("towermd trailers", "towermd --text dev trailers")
        .replace("- 只改任务卡", delegation)
        .replace(
            "- 收尾必须先提交改动，再执行 `towermd dev finish`；",
            "- 收尾前先做委托卡上的交付前自检。收尾必须先提交改动，再执行 `towermd dev finish --claims <file>`，文件按完成标准逐条写自述 `[{\"criterion\": 0, \"note\": \"…\"}]`，自述不算证据；",
        )
        .replace(
            "- 合并前执行 `towermd --text dev trailers",
            "- `dev finish` 或 `dev trailers` 返回 `gate_failed` 且原因写着“检查未通过”时，是 `.towermd/gates.yaml` 声明的检查被确定性上报阻止：修好问题，等发送者降级解除后再收尾。不要改 `gates.yaml` 或代发上报来绕过；改检查声明只能走提案。\n- 不要靠删减完成标准、验证命令或放宽门禁、阈值来“通过”。这类改动会被标为承诺弱化，计入你这一配置的信任账本；确实需要放宽时，在提案或澄清里写明理由，由人决定。\n- 合并前执行 `towermd --text dev trailers",
        );
    let rendered = render_agents_md("docs/tower", KITS);
    assert_eq!(rendered, expected);
    for required in ["命令行不提供批准类命令", "进度只认证据"] {
        assert!(rendered.contains(required), "{required}");
    }
    for obsolete in [
        "尚未提供",
        "当前能力",
        "网络远端不可用不阻止已有缓存项目的本地读取与续租",
        "后续的",
        "init` 与守护进程管理命令也按调用位置续租",
    ] {
        assert!(!rendered.contains(obsolete), "{obsolete}");
    }
}

#[test]
fn dev_init_creates_the_v0_1_1_skeleton_and_skill_hint() {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["init", "-q", "-b", "main"])
        .arg(dir.path())
        .status()
        .unwrap();
    assert!(status.success());
    let report = init(dir.path(), &InitOptions::default(), KITS).unwrap();
    assert_eq!(report.created.len(), 7);
    for sub in ["intents", "arch", "specs", "baselines", "decisions"] {
        assert!(
            dir.path().join(&report.docs_dir).join(sub).is_dir(),
            "{sub}"
        );
    }
    assert!(
        report.hints[0].contains("towermd-finish"),
        "{:?}",
        report.hints
    );
}
