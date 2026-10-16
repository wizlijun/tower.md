---
type: ImplementationPlan
status: in-progress
title: M0-3 快照、主干租约与灾难恢复
---

# M0-3 快照、主干租约与灾难恢复

前置：M0-2 独立审查和主工作区验收通过。用户要求持续推进，不重复询问执行许可；仍不为仓库代码创建提交。测试仓库中的快照提交是功能验收，不是对本仓库的提交。

## 不变量

快照由 daemon 持 main 租约经唯一写模块生成；只包含 docs_dir/state/state.json 与按月事件档案，不改其他慢层。配置来自权威 main。连续前缀以 hub 水位为准，不能以客户端最后收到的消息为准。拒绝过期 fence、并发主干推进和不确定的持久化状态。恢复不能由 CLI 自动批准，须保留 App 确认闸门；M1 App 未提供前只能请求恢复，不能用参数绕过。

## 设计任务（执行前细化 API）

2026-10-16 用户已确认：state.json 保存业务投影及 eid→seq 等最小恢复元数据；租约摘要仅追溯、灾难恢复全失效。恢复先将新 seq/epoch 边界写入权威 main，再开放服务；App 人工确认不可绕过。

- [ ] hub checkpoint / snapshot.ack：验证 SHA 与连续 seq 边界，持久保存 eid 去重索引；可先不压缩事件内容（FR-92 允许），避免引入不必要的丢失窗口。
- [ ] 状态检查点：按 Task/Run 等 type 分组；引用 seq/epoch 与格式版本；全事件重放与检查点等价测试。续租心跳不进入月档案，其租约摘要如何保证重放一致必须在实现前明确，不可默默丢字段。
- [ ] main 租约：TTL 2 分钟、30 秒续租；接任先检查 main trailer 并补 ack；旧持有者不能写入。
- [ ] Git 写模块：独立临时 index、显式进程身份、禁用 hooks/签名；只生成目标状态路径；远端在 fetch 的 origin/main 上建提交并快进推送，被拒则重新 fetch 并检查 fencing，不使用强推。
- [ ] 无远端：main 未检出用带旧值的 update-ref；已检出时一律推迟，不触碰用户工作树和 index；2026-10-17 人已批准此安全收紧，取代 commit --only 例外。
- [ ] 手动 snapshot 与有变化时 30 分钟触发；事件中记录提交、覆盖水位和 epoch。已成功写 Git、尚未 ack 的重启窗口可恢复，不能重复丢失事件。
- [ ] 恢复核心：从 main 快照和其后 trailer、发件箱恢复，seq/epoch 跳过 2^32，旧租约全作废，保留去重语义；拒绝不连续、未知格式、路径越界或无确认的请求。M0 无离线发件箱写入者，仍须为合法输入定义恢复契约。
- [ ] init 可推送性检查：dry-run 只能证明当前连接与引用协商，不能声称证明服务端所有写入策略；无法保证直接推送时清晰提示/报错，不能替用户实际推送初始化内容。
- [ ] 独立规格/质量审查，故障注入、并发 push、脏工作树、恢复后 stale epoch、乱序 103/102 水位不越过 101；全部通过才标完成。

## Git 实现依据

使用 Git 原生对象和引用协议，不引入 libgit2。API 细节以官方 [commit-tree](https://git-scm.com/docs/git-commit-tree)、[push](https://git-scm.com/docs/git-push) 与 [update-ref](https://git-scm.com/docs/git-update-ref) 文档为准；测试仅操作专用临时仓库。

## 已确认设计的工程契约

### 共享业务投影与恢复元数据

`Snapshot` 包含 `format_version`、`project_id`、`through_seq`、`writer_epoch`、`epoch_high_water`、`projection`、`receipts` 和合法省略区间。`projection.objects` 按 type、对象 ID 分组，只存当前对象，不存历史事件正文；`lease_summaries` 只记 name/holder/epoch，是记录中的归属摘要，不是有效授权。`receipts` 只记 eid→seq，包括被省略的续租心跳。writer epoch 与全局 epoch 高水位分别保存，不能互相代替。

业务 reducer 只应用已记录结果，不读当前时钟，不重新判定 stale。月归档只省略确定为纯续租、没有业务对象变更的心跳；普通事件、领取、释放、到期和恢复失效转换都归档。对比的是“全归档重放的业务投影 == snapshot.projection”；恢复元数据另行验证。月档案允许有明确声明的心跳省略和恢复跳号，除此之外缺失业务事件即拒绝。

### 本地 journal 与恢复

普通重启继续使用完整 journal，保留仍有效的租约；快照不要求立即压缩 journal。灾难恢复的 journal 增加基投影、起始 seq/epoch 与历史 receipts，随后接连续尾部事件。重复请求只需返回原 seq；只有仍保留正文时才返回完整 Event，不构造假的历史事件。

恢复准备不激活服务：核实项目、格式、trailer、归档覆盖和投影一致性，再由 App 确认；从所有可信来源的高水位 checked_add(2^32)，先把新边界写入权威 main，成功后原子安装本地恢复基线。若进程在 main 成功后崩溃，重试必须从已持久化的新边界继续，不能从旧快照再次发相同区间。记录可重放的恢复失效事件，旧 running Run 回 released、Task 回 ready，旧 lease 全失效。CLI 只创建恢复请求，不暴露批准参数。

### 唯一 Git 写模块

使用显式临时 index、进程作者/提交者身份、禁用 hooks/签名/过滤器，写入前后验证只涉及允许的两个 state 路径。命令有有限超时，网络写入不自动盲重试。远端失败重新读取 main 和 fencing 后再决定；本地 main 在任一工作树已检出时一律推迟，其他工作区与暂存内容原样保留；未检出的 main 使用不解引用符号引用的旧值校验更新。所有测试只用临时仓库。

### 推荐实施顺序

1. 共享 recorded-event reducer、纯投影导出和 receipts；验证心跳省略后等价、未知字段与费用 null。
2. hub checkpoint/ack 与连续前缀缓冲；验证乱序、历史幂等、原子持久化故障。
3. 独立 Git 写模块与 main fence；验证远端快进/被抢先、本地 checked/unchecked、用户脏改动及 hooks 隔离。
4. daemon snapshot 调度和 CLI；持有租约才能进入唯一写模块，接任补 ack。
5. 恢复准备、App 确认闸门与持久边界协议；M0 无 App 时不可实际放行恢复，核心测试使用明确的 App 确认夹具，不能新增生产绕过路径。
6. 独立规格/质量审查与完整验证，更新实际能力说明。

当前状态（2026-10-17）：本阶段快照与恢复请求实现已通过规格与质量复审，隔离工作树最终 276 项测试、fmt、Clippy（-D warnings）、build 全通过。额外审查复现的 checked-main 并发覆盖路径已按人确认删除，符号 main 防护及恢复短锁/安装前并发检查均经独立回归复审通过。生产 CLI 只生成恢复请求，实际 App 放行与离线发件箱接入仍待后续任务，不能据此宣称整个 M0 完成。所有代码未提交、未发布；主工作区同步后的验证另记 tasks/todo.md。

## 验收回顾

- [x] checkpoint/ack、业务投影、eid receipts、连续前缀及心跳归档省略。
- [x] 主干租约、手动/定期快照、Git 成功而 ack 未落盘的重启补确认。
- [x] 单一 Git 写入模块：远端快进、未检出本地 main 的 no-deref CAS、已检出 main 一律 Deferred；不写用户工作树/index。
- [x] 恢复请求及不可由 CLI 构造的 App 确认契约；测试夹具证明先持久化新边界、再安装本地状态，旧租约失效与历史幂等保留。
- [x] init dry-run 推送协商检查及其明确能力边界。
- [x] 独立规格/质量复审、故障与并发回归、276 项完整测试及格式/静态检查/构建。
- [ ] 后续阶段：实际 App 确认入口、离线发件箱生产与恢复；当前无绕过入口。
