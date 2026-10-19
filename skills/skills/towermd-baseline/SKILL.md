---
name: towermd-baseline
description: 扫描真实项目并按唯一 schema 综合 S6 草稿，提交后等待人在 App 审阅和批准首个 B0。
---

先读取[共享会话契约](../../references/session-contract.md)。

此流程只生成草稿，不出版基线。先确认用户要扫描的真实项目，在该项目目录启动一次扫描：

<!-- towermd-cli:baseline-scan -->
```sh
"$TOWERMD_CLI" baseline scan
```

保存返回的 scan ID。未结束时只等待同一 ID，每次最多 90 秒；不要重复启动来绕过失败。`SCAN_ID` 取自原返回值。

<!-- towermd-cli:baseline-scan-wait -->
```sh
"$TOWERMD_CLI" baseline scan --wait "$SCAN_ID"
```

扫描失败、证据缺失或验证 no_tests 时停止，保留原报告并说明需人工处理之处。成功后读取唯一实时 schema：

<!-- towermd-cli:baseline-schema -->
```sh
"$TOWERMD_CLI" baseline schema
```

以真实扫描材料和已有代码为依据综合 S6，不复制另一份 schema，不臆造测试通过、边界或目标。候选 JSON 和说明存工作树外的任务材料目录；schema 要求哪些引用/固定提交就使用实际值。`DRAFT_JSON` 指向该 UTF-8 JSON。

<!-- towermd-cli:baseline-submit -->
```sh
"$TOWERMD_CLI" baseline submit "$DRAFT_JSON"
```

保留返回的 request_id、draft ID 和 revision。网络结果不确定时只用原 UUIDv7、原 Run 与完全相同的 JSON 重试：

<!-- towermd-cli:baseline-submit-retry -->
```sh
"$TOWERMD_CLI" baseline submit "$DRAFT_JSON" --request-id "$REQUEST_ID"
```

请人在 App 打开同一项目并审阅该草稿，完成 S7 编辑与 S8 明确批准 B0；Agent 不调用批准端点。返回草稿 ID、固定输入提交、验证结果、待审差异与准确 App 操作入口。后续基线出版未接通时如实停车，不能把草稿保存称为 B1 已发布。
