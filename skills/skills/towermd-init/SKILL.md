---
name: towermd-init
description: 初始化已有 Git 仓库的 Tower.md 配置，保留既有项目 ID，并提示用户加载原生技能。
---

先读取[共享会话契约](../../references/session-contract.md)。

先确认目标仓库与用户指定的相对文档目录；非交互新项目不能猜目录。下列变量由真实任务参数赋值，`PROJECT` 是已有 Git 仓库内路径，`DOCS_DIR` 是用户确定的相对目录。

<!-- towermd-cli:init -->
```sh
"$TOWERMD_CLI" init "$PROJECT" --docs-dir "$DOCS_DIR"
```

读取返回的 created、warnings、hints；保留项目 ID，重复运行不覆盖已有配置。`init` 不连接 daemon，也不提交。展示生成文件和用户需接入的规则引用，由用户检查并决定提交；不要自动改 main、全局配置或安装目录。已配置项目可省略 docs-dir 沿用配置，若要变更则走提案。

随后请用户在 App 打开同一仓库；如需冷启动基线，使用 towermd-baseline 的扫描和草稿流程。安装由用户按[安装说明](../../INSTALL.md)执行，技能不替用户修改 harness 配置。
