use std::process::Command;

use towermd_core::init::{init, render_agents_md, InitOptions};
use towermd_devkit::dev_state::KITS;

#[test]
fn dev_agents_md_is_byte_identical_to_v0_1_1() {
    let expected = include_str!("fixtures/agents-v0.1.1.md").replace("{docs_dir}", "docs/tower");
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
