---
name: towermd-seed
description: 用一句话（或人在 App 中立项的项目目标）生长 Tower.md 项目的第一个版本：澄清不超过 3 个问题，写 1–3 个模块的最小骨架，提交与冷启动 S6 相同的草稿基线，交由人在 App 批准 B0。已有批准版本时不用本技能，改用提案生长。
---

# 一句话生长

只用于还没有经批准版本的项目。使用当前会话可用的最强模型；Tower.md 不代选或启动 harness，不要更改全局 harness 配置。

1. 目录还不是 Git 仓库时，先在该目录运行 `git init`，再运行 `towermd init --docs-dir docs` 并提交它生成的启用配置。不改全局 git 配置。
2. 调用 `towermd dev seed "<那一句话>"`；人已在 App 把想法立项为项目目标时，直接 `towermd dev seed`。返回的 `data` 是种子计划：`sentence`、草稿意图 `intent`（`I-1`，L0，`path` 为 `intents/I-1.md`，`text` 是 OKF 原文）、上限 `limits` 与步骤 `steps`。错误码为 `needs_proposal` 表示项目已有批准版本，改用 [towermd-propose](../towermd-propose/SKILL.md)；`usage` 表示没有这句话也没有立项目标，或这句话不是单行 200 字以内，请人补上。种子计划只读，不写任何内容。
3. **澄清**：最多 `limits.questions` 个问题，只问会改变模块划分或第一个任务的事。人在会话里时直接问；需要人在审阅时看到的问题写进步骤 5 的 `questions`。不要为凑数提问。
4. **骨架**：按 `limits.modules`（1–3 个模块）写能构建的最小代码和一条冒烟测试，作为普通提交合入 `main`。每一步结束时 `main` 都应能通过验证；不要先写大量功能。
5. **草稿基线**：按 [towermd-baseline](../towermd-baseline/SKILL.md) 的第 1–5 步扫描骨架提交并提交综合草稿：`towermd dev scan start`，`towermd dev scan wait <scan-id>`，`towermd --text dev draft template <scan-id>` 取模板。以 `intent.text` 为 `intents/I-1.md` 的原文放进 `intents`，只把其 frontmatter 的 `allocated_to` 填成它落到的模块 ID，句子本身不改；每个模块写验收标准。然后 `towermd dev draft submit <json-file> --producer <harness/version>`。
6. **批准**：告诉人到 App 审阅并批准 B0。Agent 不能批准，CLI 也没有批准命令；批准前不要宣称版本已生效。
7. **任务**：B0 批准后，按骨架用 `towermd task add <task.json>`（输入格式见 `towermd schema task.add`）添加不超过 `limits.tasks` 个任务，第一个是 `limits.first_task`（“空骨架能构建并通过一条冒烟测试”），完成标准写可复核的命令或现象。之后按 [towermd-claim](../towermd-claim/SKILL.md) 领取。

之后的生长靠证据和提案：Tower.md 在主干上测得生长信号（模块过大、跨模块任务过多、无主文件、意图未分配、同类错误反复）时会在 App 关注清单里提示；人让会话中的 Agent 起草提案，由人在 App 批准或驳回。信号本身不改变任何结构。
