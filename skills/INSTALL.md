# Tower.md 原生技能安装

本目录是技能包根，包含 `.claude-plugin/plugin.json`、`skills/` 与 `references/`。六项技能共用同一正文：初始化、首个基线草稿、领取、问答/评审、提案、验证收尾。当前为开发版本；**propose 已提供源绑定私有提案提交；后续基线出版仍待完成，相关执行须安全停车，完整 M1 尚未验收。** 不含 seed、无头运行器、hooks 或 MCP。

## 准备真实路径

源码使用时，`TOWERMD_SKILLS_ROOT` 指向本仓库的 `skills/` 目录。发行包先核对随包 SHA256SUMS 再解包；路径应是 `towermd-skills-<version>/skills/`，不是再往下的 `skills/skills/`。

`TOWERMD_CLI` 指向实际 App 的 `Contents/MacOS/towermd`，`TASK_TREE` 指向用户要操作的项目工作树。用当前真实路径替换下面示例后，在终端设置：

```sh
export TOWERMD_SKILLS_ROOT="/absolute/path/to/tower.md/skills"
export TOWERMD_CLI="/absolute/path/to/Tower.md.app/Contents/MacOS/towermd"
export TASK_TREE="/absolute/path/to/task-worktree"
test -f "$TOWERMD_SKILLS_ROOT/.claude-plugin/plugin.json"
test -x "$TOWERMD_CLI"
```

<!-- towermd-cli:version -->
```sh
"$TOWERMD_CLI" --version
```
<!-- towermd-cli:help -->
```sh
"$TOWERMD_CLI" --help
```

路径、版本与包来源要一致。不要安装到全局 PATH，不读取认证文件。会话未继承环境变量时，在该会话明确提供同一个绝对 CLI 路径，先验证可执行，不依赖隐含环境转发。

## Claude Code：仅本次会话加载

在目标项目工作树，由用户启动自己的真实 Claude Code 会话：

```sh
cd "$TASK_TREE"
claude --plugin-dir "$TOWERMD_SKILLS_ROOT"
```

在技能选择器核实实际来源，调用 `/towermd:towermd-claim`，或按任务阶段调用同命名空间的其他技能。已有会话若不支持动态加载，按同一目录重开/恢复；不改永久插件设置。名称冲突时核对技能路径，不使用来源不明的同名项。

## Codex：手动链接项目技能

Codex 从项目 `.agents/skills/` 发现技能；将同一包的六个目录链接进目标工作树。用户先确认该项目的本机技能链接应如何忽略：可按项目约定手动添加忽略规则并检查 Git 差异；本包不会自动改规则。否则链接成为 untracked，会阻止 finish 的干净树门禁。

以下步骤由用户在目标工作树执行；若任何同名路径或链接已存在，停止并核对，不覆盖。此步骤只安装项目链接，不启动任务、领取 Run 或修改全局配置。

```sh
cd "$TASK_TREE"
mkdir -p .agents/skills
for name in init baseline claim ask propose finish; do
  target=".agents/skills/towermd-$name"
  test ! -e "$target" && test ! -L "$target" || exit 1
  test -f "$TOWERMD_SKILLS_ROOT/skills/towermd-$name/SKILL.md" || exit 1
done
for name in init baseline claim ask propose finish; do
  ln -s "$TOWERMD_SKILLS_ROOT/skills/towermd-$name" ".agents/skills/towermd-$name" || exit 1
done
git status --porcelain=v2 --untracked-files=all
codex -C "$TASK_TREE"
```

在 `/skills` 确认六项及其实际路径，调用 `$towermd-claim`。每个工作树分别安装；不要在仍绑定 Run 时搬动目录或分支。如果中途失败，列出实际链接，手动只移除确实指向本包的链接，再重试；不递归删目录。

## 首次任务与校验

新项目先用 towermd-init，按提示检查初始化内容并提交；然后 towermd-baseline 扫描、综合草稿，请人在 App 审阅和明确批准 B0。已有基线项目可直接领取已登记任务。按阶段使用 towermd-ask / towermd-finish；需要慢层变更则 towermd-propose 当前会说明缺入口并停车。

给真实会话提供任务 ID、目标树、精确 CLI 路径及明确授权范围。检查点读取状态；完成只认实际验证与 main 认证，不认会话自述。

开发者可运行只读校验（需要 Node，但运行 App 和技能业务 CLI 不需要 Node）：

```sh
node "$TOWERMD_SKILLS_ROOT/scripts/check-contracts.mjs" --root "$TOWERMD_SKILLS_ROOT" --cli "$TOWERMD_CLI"
```

校验只运行 `--help`/`--version` 探针，不执行 init/claim/ask/finish 等业务。当前应报告 propose 未接入并退出非零，这代表完整六技能尚未发行就绪；不能删掉检查来把部分交付当完整验收。自动校验也不能证明两个真实 harness 已加载技能，或替代真人 App 放行与十任务验收。

## 更新与卸载

更新先比较包版本和六项正文 hash，核对随包 CLI 契约；既有同名文件/链接不覆盖。项目已有活动 Run 时先保留当前环境，不悄悄切换技能包。卸载 Codex 项目链接时逐个查看 `readlink`，只手动删除精确指向本包的链接，保留用户文件与其他技能；Claude Code 下次启动不传 `--plugin-dir` 即不加载本包。不要修改全局 Git/harness 设置。

原生加载依据：[Codex skills](https://learn.chatgpt.com/docs/build-skills)、[Claude Code skills](https://code.claude.com/docs/en/skills)、[Claude 插件清单](https://code.claude.com/docs/en/plugins-reference)。目标 harness 版本改变时重新核对其实际 help 和选择器；本包不把安装说明当作实测证据。
