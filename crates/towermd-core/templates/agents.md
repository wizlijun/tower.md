# Tower.md 仓库规矩

本文件由 `towermd init` 生成，写给在本仓库里工作的任何 Agent。修改本文件属于 L3 流程变更，需要人登记。

## 当前能力

`towermd init` 在当前工作树生成配置和文档骨架，供人检查并自行提交；它不 add、不 commit。`status` 与 `task add|claim|show|release` 通过本用户守护进程访问持久化本地 hub，只使用权威 main 的配置、模块、验收和决策。快照、App 批准与合并拦截器尚未提供；不要自行修改受保护内容，请人检查处理，也不要删除 hub 数据。

## L0–L2 慢层：只能经人在 Tower.md App 中批准修改

- `{docs_dir}/intents/`、`{docs_dir}/arch/`、`{docs_dir}/specs/`、`{docs_dir}/baselines/` 下的全部文件
- `{docs_dir}/tower.canvas`
- 其他位置 frontmatter 中 `layer` 为 `L0`–`L2` 的对象

需要改慢层时，不要直接改。后续通过 `towermd propose` 提出提案并附上 diff，经人批准后由 Tower.md 写进主干。当前尚未提供该命令，也没有自动拦截所有合并的门禁。

## 快照专用

`{docs_dir}/state/` 只由持有主干租约的 Tower.md 守护进程经快照写入模块创建和写入。`init` 不创建它；不要手写快照，也不要伪造事件。

## L3 配置与流程

`.towermd/` 下的配置及本文件属于 L3 配置与流程，不归为 L0–L2 慢层。变更需要人登记；`init` 沿用已有项目 ID 和配置，不覆盖人工修改的规矩。

## 执行状态、完成与合并

任务、领取与租约已由 Tower.md hub 保存，通过守护进程访问。先创建目标工作树和 `t/<task-id>` 分支，再运行 `towermd task claim <task-id>`；`task show` 自动按当前工作树和分支选择运行。用户命令为调用工作树的绑定运行续租（TTL 15 分钟），长操作前后调用 `status`；`init` 与守护进程管理命令也按调用位置续租。后续的 `towermd ask`、`towermd propose`、`towermd finish` 尚未提供，命令行不提供批准类命令。

- 只改任务卡 `allowed_paths` 范围内的文件。
- 后续合并前须运行 `towermd finish`，证据通过后才能合并；当前须由人检查和执行合并。
- Agent 自己报告“完成”不会改变任务状态，进度只认证据。
