---
type: ImplementationPlan
status: stable
title: M0-2 本地 hub、守护进程和任务命令
---

# M0-2 本地 hub、守护进程和任务命令

执行方式：superpowers，测试先行，独立规格审查后质量审查。用户已授权持续推进；不创建分支、不自动提交。M0-1 已验证实现作为基线，在 detached worktree 隔离开发。

## 范围与约束

- 核心 crate 实现事件、租约、任务投影和权威对象读取；CLI/daemon 只适配。
- 唯一用户级 daemon，监听 127.0.0.1；所有项目请求并发进入，单项目写入串行。
- 本地 hub 位于 canonical git-common-dir/towermd，同 clone 工作树共享；runtime 项目锁拒绝同 ID 的独立 clone。
- 配置与慢层对象只读 fetch 后 origin/main（无 origin 则 main）的提交树。任务命令不写工作树或用户文档目录。
- journal 为原子替换的持久化事件集合；每次先构造候选状态、同步持久化，再交换内存。未实现压缩前保留全部 eid，投影可重放。大规模日志优化不属于本次。
- 每个业务事件可携带一组原子租约变更，claim 不允许部分成功；旧 fence 事件记录 stale，不应用投影或租约。
- 模块租约 TTL 15 分钟，每次绑定命令续租；事件保存接收时的 stale 判定，重放不根据当前时钟重新判定历史。
- CLI 固定 process:towermd-cli，不开放任意事件、human 署名、批准、完成或绕过门禁的入口。未知费用 null。
- M0-3 负责快照、压缩确认和恢复；M0-4 负责 canvas/coldstart，本计划不冒称整个 M0 完成。

## 已确认的产品语义（2026-10-16）

1. 没有相关 Decision 时允许；显式引用必须存在于权威 main。
2. 先限定在已有 t/<task-id> 分支、目标工作树领取，暂不支持迁移。
3. 实际修改范围取任务 allowed_paths，模块 paths 仅为上限。

用户已逐项选择上述规则；不再作为未决项，不扩大到 M1 的完整门禁与提案批准。

## 依赖

沿用 Note.md：serde 1 derive、serde_json 1、chrono 0.4 clock/serde、fs2 0.4、dirs 5、sha2 0.10、hex 0.4、tokio 1、reqwest 0.12（blocking/json、关闭默认 TLS）。axum 0.8 为已批准 HTTP 选型。uuid 1 v7 已存在。没有新增未批准库。

## 执行清单

- [x] 1. core/hub.rs：先写序号、去重、重启、拒收不占号、原子租约和 stale 测试，再实现 journal、单写者锁和 reducer。
- [x] 2. core/catalog.rs：先写工作树篡改无效、origin/main、linked worktree、未知对象宽容读取、重复 ID 拒绝、路径上限测试，再实现只读提交树加载。
- [x] 3. core/tasks.rs：先写任务创建/领取/释放/到期/重放及绑定测试，再实现事件驱动投影与 G0 最小硬条件；三项语义已获人确认。
- [x] 4. daemon/client：先写多进程首次启动、多个项目隔离、同 ID 异 clone 拒绝及重启验收，再实现 runtime 锁、loopback HTTP 与有限等待。
- [x] 5. CLI：status、task add/claim/show/release、--run；默认 JSON 输出确保可供技能调用；保留 init 行为与所有回归。
- [x] 6. 独立规格审查，再独立质量审查；修复必须先加回归。
- [x] 7. cargo test --workspace --locked、fmt --check、clippy --all-targets -D warnings、build；更新 README、双语未发布日志与任务回顾，精确同步，不提交。

## 验收

同 clone 两工作树争领仅一个成功；多模块全部成功或全不发生；客户端退出服务仍活着；有效期与续租重启后不丢；切换分支不误绑定，同工作树不能第二个活动 run；失效 epoch 保留事件但不改变投影；失败落盘不更新内存；工作树文件、Git index、分支与用户全局配置保持不变。

## 审查修复记录

独立规格审查提出的六项缺口均已补实现及回归：权威读取禁用 Git replacement refs、固定路径之外的慢层对象也受保护、任务卡展开执行上下文、harness 信息只记 self_reported、WebSocket 持续订阅、init/daemon 生命周期命令按调用 cwd 续租。额外修复普通非对象 frontmatter 被错误拒绝的问题。

规格复审 PASS；质量审查补两项红绿回归后 PASS：daemon 与首个客户端隔离进程组，Ctrl-C 不杀共享服务；stop 携带无效显式 --run 时拒绝，隐式续租失败才允许告警后停机。主工作区同步及最终验收通过：163 项测试，fmt、Clippy（-D warnings）与 build 全部通过。当前未提交、未发布；无新增提交号。
