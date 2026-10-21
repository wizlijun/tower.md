# Tower.md 仓库规矩

本文件由 `towermd init` 生成，写给在本仓库里工作的任何 Agent。修改本文件属于 L3 流程变更，需要人登记。

## L0–L2 慢层：只能经人在 Tower.md App 中批准修改

- `{docs_dir}/intents/`、`{docs_dir}/arch/`、`{docs_dir}/specs/`、`{docs_dir}/baselines/` 下的全部文件
- `{docs_dir}/tower.canvas`
- 其他位置 frontmatter 中 `layer` 为 `L0`–`L2` 的对象

需要改慢层时，提出附带 diff 的提案，经人批准后由 Tower.md 写入主干。不得直接修改或绕过批准。

## 快照专用

`{docs_dir}/state/` 只由持有主干租约的 Tower.md 守护进程经快照写入模块创建和写入。`init` 不创建它；不要手写快照、伪造事件或自行删除 hub 数据。

## L3 配置与流程

`.towermd/` 下的配置及本文件属于 L3 配置与流程，不归为 L0–L2 慢层。变更需要人登记；`init` 沿用已有项目 ID 和配置，不覆盖人工修改的规矩。`init` 只在当前工作树生成文件，不提交，不为运行续租。

