---
name: towermd-init
description: 在已有 Git 仓库初始化 Tower.md，并交接冷启动综合与 App 人工批准。用于首次接入或重复检查配置，不创建或批准基线。
---

# 初始化项目

1. 使用用户指定的仓库和文档目录。非交互初始化必须明确传入目录：`towermd init <repository-path> --docs-dir <relative-docs-directory>`。已有配置重复初始化时沿用原项目 ID；不要为解决报错手工重建 ID。
2. 阅读输出的 `created`、`warnings`、`hints`。命令只创建工作树文件，不提交、不批准基线，也不要求守护进程或网络可用。推送预检警告不等于初始化失败。
3. 让用户审阅生成文件；在用户已授权提交初始化时，精确提交这些文件作为项目启用起点。不要改全局 Git 或 harness 配置，不覆盖用户原有 `AGENTS.md`。
4. 按 [技能安装说明](../README.md) 让用户在自己的 harness 中加载技能；Tower.md 不自动安装。对现有仓库继续用 [towermd-baseline](../towermd-baseline/SKILL.md) 从固定提交生成综合草稿。
5. 交接人在 App 中审阅并批准 B0。初始化和扫描成功都不表示 B0 已批准；在真实基线出版前，不领取执行任务。

项目文档、配置、快照的写入通过 `towermd`；不要手写 `.towermd/`、用户文档对象或 hub 数据。
