---
type: Plan
title: U-6 性能与时延验收记录
status: active
generated: { by: claude-code/opus-5.5, at: 2026-10-24T12:00:00Z }
sources:
  - id: core
    resource: ../../specs/2026-10-21-spec-towermd-core.md
    title: Tower.md 核心规格（NFR-03、NFR-08）
  - id: devkit
    resource: ../../specs/2026-10-21-spec-towermd-development-kit.md
    title: Tower.md 开发套件规格（冷启动 S1–S6 时限）
  - id: cli
    resource: ../../specs/2026-10-21-spec-towermd-cli.md
    title: Tower.md 命令行规格（wait 上限与退出码）
  - id: gap
    resource: 2026-10-23-gap-core-spec.md
    title: 核心规格差距分析（U-6 行）
---

# U-6 性能与时延验收记录

本文记录 U-6 的验收结果：先列出三份规格里所有带具体数字的性能指标，再逐项给出测量方法、实测值和结论。不达标的项写明根因和建议方案。

本轮没有修改任何功能代码，只新增了测量：

- `crates/towermd/tests/perf_cli.rs`：一个默认测试和两个 `#[ignore]` 的大规模测量。
- `src/perf.test.ts`：vitest 测量。

测量只用 `std::time::Instant` 和 `performance.now()`，没有引入基准库。

## 机器与条件

- Apple M5（10 核）、24 GB 内存、macOS 27.0.1（26A434）
- rustc 1.99.1、Node v26.9.0、git 2.54.0（Apple Git-157）
- 测量期间，同一台机器上还有其他工作树的 Agent 和守护进程在编译、跑测试。数值存在噪声，偏保守。
- Rust 部分分别用 debug 和 release 构建测量，表中注明。前端部分在 vitest + happy-dom 中测量，不是真实 WebView。

## 可测指标清单

| ID | 数字 | 范围 | 来源 |
| --- | --- | --- | --- |
| NFR-03a | ≤ 2 s | 事件写入到画布显示（单机；多机时含 hub 转发） | 核心规格 NFR-03 |
| NFR-03b | ≤ 1 s | Z0 首屏渲染，50 个容器、500 个任务 | 核心规格 NFR-03 |
| NFR-08a | ≤ 5 s | 中断从发生到出现在画布 | 核心规格 NFR-08 |
| NFR-08b | ≤ 5 s | 回答到达实时等待中的受托者 | 核心规格 NFR-08 |
| DK-冷启动 | ≤ 15 min | 20 万行以内的仓库，S1–S6 完成 | 开发套件规格 §冷启动 |
| CLI-wait | ≤ 90 s（实现 85 s），超时退出码 5 | `towermd wait` 单次长轮询 | 命令行规格 |
| FR-50/151 | 1 s 合并 | 容器卡片内容变化的合并窗口 | 核心规格 |
| FR-46 | 300–800 ms | 缩放层级过渡动画 | 核心规格 |
| A.2 PlantUML | 2 s、1 MB/10 s | PlantUML 渲染时限与输出上限 | 核心规格附录 A.2 |

NFR-02、NFR-12 带有数字，但它们是正确性约束（如"100% 不进入投影"），不是时延指标，已由现有恢复和租约测试覆盖，不在本文范围内。

## 结果

"规格规模"指 500 个任务（由 `towermd-devkit` 的提案批准生成）、50 个 bound 看板容器（每个绑定 10 个任务），以及 3000 条 `report` 事件。`TOWERMD_PERF_*` 环境变量可以调整规模。

| 指标 | 场景 | 实测（p50 / max） | 结论 |
| --- | --- | --- | --- |
| NFR-03a 事件 → 实时状态 | 默认测试，空项目，debug | 532 / 598 ms | 通过 |
| NFR-03a | 200/10/600，release | 485 / 610 ms | 通过 |
| NFR-03a | 规格规模，release | 630 / 638 ms（另一轮 703 / 1623 ms） | 通过 |
| NFR-03b Z0 首屏（前端） | 合成 50 容器 / 500 张卡 | 130–188 / 155–257 ms | 通过 |
| NFR-03b Z0 首屏（前端） | 规格规模守护进程导出的真实状态 | 73 / 82 ms | 通过 |
| NFR-03b 后端份额：读视图 | 规格规模，release | 145–218 / 145–254 ms | 通过 |
| NFR-03b 后端份额：首个状态帧 | 规格规模，release | 184 / 197 ms；另一轮 260 / **42811 ms** | **不稳定，见遗留 3** |
| 状态帧 → 画布重绘（前端） | 规格规模真实状态 | 1–2 ms | 记录 |
| NFR-08a 提问 → 实时状态 | 默认测试，空项目，debug | 864 ms | 通过 |
| NFR-08a | 200/10/600，release | 3677 / **5830 ms** | **不通过** |
| NFR-08a | 规格规模，release | 第 1 个样本 **> 180 s** 未到达 | **不通过** |
| NFR-08b 回答 → 等待中的 CLI | 默认测试，空项目，debug | 2411 ms | 通过 |
| NFR-08b | 200/10/600，release | 5478 / **9646 ms** | **不通过** |
| NFR-08b | 规格规模，release（更早一轮） | **> 90 s**，wait 超时 | **不通过** |
| 回答请求本身（分解） | 200/10/600，release | 2371 / 6628 ms | 记录 |
| DK-冷启动 S1–S5 | 20 万行 / 2000 个文件，release | 1510 ms | 通过（S6 不含，见下） |
| CLI-wait 上限与退出码 5 | — | 现有 CLI 测试覆盖 | 通过 |
| FR-50/151 1 s 合并 | — | 现有 `frames`、`App` 测试覆盖 | 通过 |
| FR-46 过渡 | — | CSS 动画，视觉验收 | 不做自动测量 |
| A.2 PlantUML | — | 尚未实现 | 不适用 |
| NFR-03a 多机经 hub | — | 依赖 I-A，第一阶段无多机 hub | 不适用 |

规格规模下的其他分解数据（release）：

- CLI `signal` 往返（含进程启动）：577 / 833 ms
- HTTP 追加：312 / 317 ms
- 状态帧大小：1,961,500 字节

### 补充说明

- **NFR-03a 的画布可见延迟。** 实测值是事件到达 App 订阅的时延。容器卡片内容变化按 FR-50/151 还要再合并 1 s，所以规格规模下卡片内容的最坏可见时延约为 1.6 + 1 ≈ 2.6 s。位置、标题和角标变化会立即显示。规格把这 1 s 合并定为有意的设计，本文不把它算作超标，但这是两条规格之间的张力，需要人裁定"显示"是否包括合并窗口。
- **NFR-03b 的范围。** happy-dom 只计 Svelte 组件的创建和 DOM 构建，不计 WebView 的布局与绘制。真实 WebView 的首屏还需要在桌面 App 中实机确认。
- **DK-冷启动。** S1–S5 是守护进程自己的机械部分（`dev scan`），测试命令是一个空命令。S6 是用户自己 Agent 的一次只读运行，时长取决于模型，不在本机测量范围内。按 1.5 s 的实测值，留给 S6 的预算超过 14 分钟。
- **规模下时延持续增长。** 同一轮里，回答时延逐个样本上升，例如 debug 40/4/100 下从 1431 ms 涨到 5541 ms。每个回答都会产生一次快照提交，下面的根因 2 和 4 会随提交数累积。

## 根因（macOS `sample` 采样）

提问、回答、`wait` 轮询、消息和状态读取都要拿 `project.runtime` 互斥锁，下面几项都在持锁期间执行：

1. **发件箱全量读取。** `DurableOutbox::pending()` 每次都调用 `read_all()`，读取并校验所有待发和已确认记录的 sha，复杂度 O(N)，`find` 还会带来 O(N²)。几乎每个 CLI 写处理的 `original()` 和 `known()` 都会调用它，而发件箱从不清理。
2. **每次快照都触发完整的 main 检查和重放。** 回答处理会调用 `runtime.snapshot(now)`，在 main 上产生一次提交。事件观察者（`event_service.rs`）看到 ref 变化后，在持锁状态下调用 `finish::maintain_project`，依次执行 `inspect_catalog`、`main_inspection::inspect`、`snapshot_publication::prove`、快照读取与校验，最后完整执行一次 `project_runtime::replay`。
3. **重放约为 O(事件数 × 任务数)。** `towermd_devkit::dev_state::reduce` 处理每条 `task.*` 和 `lease.*` 事件时，都要把整个 Task/Run 映射反序列化、修改、再写回。另外，观察者首次探测也走这条路径，所以订阅后的第一个状态帧偶尔会被拖到 40 s 以上。
4. **`main_writer::history` 遍历全部历史。** 每次写快照都对 main 的完整历史执行一次 `git log`，复杂度 O(提交数)。
5. **全量状态帧。** seq 每变化一次，就推送一次完整状态，规格规模下约 2 MB。
6. **任务新增重复证明基线。** 每次新增任务都会执行 `verified_baseline` 和 `prove_commit`，这只影响造数据的速度。

## 修复与遗留

本轮**没有修改功能代码**。上面的根因都在核心或开发套件的设计层，而这些代码正由并行的线（M2-C/D/G、P2-H、P3）修改。其中第 2 项还涉及"慢层保护不可绕过"这条红线，任何改动都必须经过评审，不属于最小修复。下列遗留按收益排序：

| 遗留 | 影响 | 建议方案 |
| --- | --- | --- |
| 1. 守护进程自己的快照提交触发完整重放 | NFR-08a/b、首个状态帧 | 观察者能识别守护进程自己写入的快照提交（提交号由写入模块登记），据此跳过重放，只对外来提交做 main 检查。检查放到锁外，只在提交结果时短暂持锁。这一项涉及慢层检查路径，需要先写设计并经人确认 |
| 2. 开发套件 reduce 全表往返 | 重放与追加 | `dev_state::reduce` 只读写受影响的那一条任务和运行记录，使重放变为 O(事件数) |
| 3. 发件箱全量读取 | 每个写请求 | `pending()` 只列文件名，只读取未确认的记录；按 eid 查找走一个索引文件；已确认且已被快照覆盖的记录定期清理（索引须能重建） |
| 4. `history` 全量 `git log` | 每次快照 | 从上一个已知快照提交开始，只看增量范围 |
| 5. 全量状态帧 | 带宽与前端解析 | 首帧发全量，此后按对象发增量，并带 seq 供客户端校验 |
| 6. 新增任务重复证明基线 | 批量建任务 | 按基线提交缓存证明结果 |

建议在完成 1–3 后，重新运行规格规模的测量，确认通过后再把 NFR-08 标为已验收。

## 如何运行

默认测试是 CI 路径，空项目，约 20 s：

```sh
cargo test -p towermd --test perf_cli -- --nocapture
pnpm exec vitest run src/perf.test.ts --silent=false
```

规格规模测量（默认忽略；造数据约 15 分钟，整轮约 20 分钟）：

```sh
cargo test --release -p towermd --test perf_cli scale -- --ignored --nocapture
# 调整规模与样本数，并导出状态供前端复测：
TOWERMD_PERF_TASKS=200 TOWERMD_PERF_FRAMES=10 TOWERMD_PERF_EVENTS=600 TOWERMD_PERF_SAMPLES=5 \
TOWERMD_PERF_STATE_OUT=/tmp/towermd-state.json \
  cargo test --release -p towermd --test perf_cli scale -- --ignored --nocapture
TOWERMD_PERF_STATE=/tmp/towermd-state.json pnpm exec vitest run src/perf.test.ts --silent=false
```

冷启动测量（默认忽略；`TOWERMD_PERF_LINES` 默认 200000）：

```sh
cargo test --release -p towermd --test perf_cli cold_start -- --ignored --nocapture
```

每个指标输出一行 `PERF <名称> n= p50= p95= max= limit= PASS|FAIL`。断言只针对 NFR 指标，分解项只输出、不断言。规模测量当前会因 NFR-08 超标而失败，这是预期结果，遗留修复后应转为通过。
