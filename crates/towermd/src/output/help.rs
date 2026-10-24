//! Localized about/help texts and examples, applied to the clap tree at runtime.
use super::lang::{pick_in, Lang};
use clap::Command;

/// (command path, en, zh); "" is the root.
const ABOUT: &[(&str, &str, &str)] = &[
    ("", "Tower.md command line: delegates do the work, you approve only where it matters.", "Tower.md 命令行：让受托者去做，只在关键处放行。"),
    ("up", "Start the built App and the local daemon", "启动已构建的 App 和本机守护进程"),
    ("show", "Show any object by ID (task, run, question, proposal, decision, frame…); without an ID, the project canvas", "按 ID 显示任何对象（任务、运行、提问、提案、决策、容器…）；省略 ID 时为项目画布"),
    ("needs", "The attention list (Needs You) in the daemon's order: the first screen, at most 3 items", "关注清单（Needs You），按守护进程的顺序：首屏，最多 3 条"),
    ("since", "What changed since your last look (or --from), in three lines", "自上次查看（或 --from）以来的变化，3 行"),
    ("overview", "All projects this user ran towermd in, with what needs attention; never starts the daemon", "本用户运行过 towermd 的全部项目及其待处理数；不启动守护进程"),
    ("ls", "List one kind of object, optionally by state or time", "列出某类对象，可按状态或时间筛选"),
    ("history", "An object's timeline: event metadata only, oldest first", "对象的时间线：只列事件元数据，按时间先后"),
    ("watch", "Follow the event stream; one JSON envelope per line; reads only and delivers nothing", "持续输出事件流；JSON 模式下每行一个信封；只读，不送达消息"),
    ("open", "Open the project in the App; locating a given object is not supported yet", "在 App 中打开项目；暂不支持定位到指定对象"),
    ("decision", "Read decisions; retiring one is a person's action in the App", "查看决策；废止决策是人在 App 中的动作"),
    ("decision.show", "Show one decision", "查看一条决策"),
    ("decision.ls", "List decisions recorded from answers and decision documents", "列出由回答记下的决策和决策文档"),
    ("rule", "Read the rules in force; adding one is a person's action in the App", "查看生效中的规则；新增规则是人在 App 中的动作"),
    ("rule.ls", "List rules registered from answers and rules set per work kind", "列出由回答登记的规则和按工作类别设定的规则"),
    ("rebuild-index", "Rebuild this machine's derived indexes; v0.1 keeps none, the daemon rebuilds state from snapshots", "重建本机派生索引；v0.1 没有单独的索引，守护进程从快照重建状态"),
    ("doctor", "Check this machine: git, the daemon, the project's hub; local only, nothing is uploaded or started", "本机自检：git、守护进程、项目的 hub；只在本机输出，不上传，不启动任何进程"),
    ("whoami", "Show who this call is recorded as: run, self-reported identity, and that it cannot act as the person", "显示本次调用会被记成谁：运行、自报身份，以及不能代人做动作"),
    ("completion", "Print a shell completion script", "输出 shell 补全脚本"),
    ("task", "Manage tasks and worktree runs (only the App approves versions)", "管理任务与工作树运行（只有 App 能批准版本）"),
    ("task.add", "Create a task from JSON; its paths must stay within the approved module grants", "从 JSON 新建任务，路径必须落在已批准模块授权上限内"),
    ("task.edit", "Replace an unclaimed task the command line created (same JSON as add, same ID); a card the person finalized is not editable", "替换一个由命令行创建、尚未领取的任务（与 add 相同的 JSON、相同 ID）；人已定稿的委托卡不可改"),
    ("task.ls", "List tasks, optionally by state", "列出任务，可按状态筛选"),
    ("task.claim", "Claim a task in the t/<task-id> worktree; its module stays yours until released or expired; without an ID, only when exactly one task can be claimed", "在 t/<task-id> 工作树领取任务，并占用所属模块直到释放或到期；省略 ID 时仅在恰好一个任务可领取时领取"),
    ("task.show", "Show a task; without an ID, look it up from the current worktree or --run", "查看指定任务；省略 ID 时按当前工作树或 --run 查找"),
    ("task.release", "Release the current run or the one given by --run; the task goes back to ready", "释放当前或 --run 指定的运行；任务回到就绪"),
    ("propose", "Submit a proposal (JSON) to change what people set, for a real draft: reasons, alternatives and a recommendation; only the App approves", "提交真实草稿的人定内容变更提案（JSON）：理由、备选方案和建议；App 才能批准"),
    ("pause", "Stop admitting new tasks to the project and send a checkpoint instruction to running delegates", "暂停项目新任务准入，并向运行中的受托者送达检查点指令"),
    ("resume", "Ask the person in the App to lift the project pause; the command line never resumes by itself", "请求 App 中的人解除项目暂停；命令行不会自行恢复"),
    ("status", "Query the project state and current run on authoritative main (JSON), and renew your claim on the module", "查询权威主干上的项目状态与当前运行（JSON），并延续模块占用"),
    ("ask", "Ask the person in the App and pause the affected tasks; never picks an answer by itself", "向 App 提问并暂停受影响任务；不自动选择答案"),
    ("wait", "Wait for an answer (H-…) or a verification (E-…), at most 85 seconds per call; a question is parked after ten minutes in total", "等待问题答复（H-…）或验证结果（E-…），单次最多 85 秒；问题十分钟累计上限后挂起"),
    ("answer", "Relay what the person answered in this session; never recorded as a decision, a rule or an approval of what people set", "转述人在会话中的本次回答；不能记为决策、规则或对人定内容的批准"),
    ("handled", "Say you acted on a prompt delivered to this run; the person sees it as handled, which is never progress or an approval", "说明你已处理送达本运行的一条插话；人会看到\u{201c}已处理\u{201d}，它不算进度，也不是批准"),
    ("reply", "Answer a report request or node question delivered to you; a report never counts as progress", "回应送达给你的汇报请求或节点提问；汇报不算进度"),
    ("report", "Report progress to Tower.md, in a run or from any script; a report is a claim and never completes anything", "向 Tower.md 汇报进度，运行中或任意脚本均可；汇报只是自述，不会让任何事完成"),
    ("signal", "Raise something worth noticing, with a level; shown as an Update or an Alert, Tower.md decides when to interrupt", "上报值得注意的事并声明级别；界面显示为动态或提醒，何时打扰由 Tower.md 决定"),
    ("idea", "Jot down ideas; they stay leads until a person promotes one in the App", "记下想法；人在 App 中立项之前，它们只是线索"),
    ("idea.add", "Jot down an unpromoted idea instead of creating a task; it is not in the attention list and nobody can claim it until a person promotes it in the App", "记下一个未立项的想法，而不是直接建任务；人在 App 中立项之前，它不进关注清单，也不能被领取"),
    ("ext", "Read the extensions installed on authoritative main; installing or changing one is a proposal", "查看权威主干上已安装的扩展；安装或修改扩展须经提案"),
    ("ext.ls", "List installed extensions, their declared signal kinds and any manifest problems", "列出已安装的扩展、它们声明的信号种类和清单问题"),
    ("ext.show", "Show one installed extension's manifest; an extension with a problem cannot send anything", "查看一个已安装扩展的清单；清单有问题的扩展不能发送任何消息"),
    ("template", "Read the templates a person can place on the canvas in the App", "查看人可以在 App 中放上画布的模板"),
    ("template.ls", "List starter templates and those installed on authoritative main, with their layout and any manifest problems", "列出起步模板和权威主干上已安装的模板，以及它们的布局和清单问题"),
    ("frame", "Read frames and write their cards by the frame's rules; only a person creates, moves or resizes a frame or changes its rules, in the App", "查看容器，并按容器规则写卡片；创建、移动、缩放容器和改规则只由人在 App 中进行"),
    ("frame.show", "Print a frame's rules, template, layout, cards and waiting items compactly, for a delegate to read before writing cards", "紧凑打印一个容器的规则、模板、布局、卡片和待人处理的项，供受托者写卡片前读取"),
    ("frame.add", "Add a card to a frame; a write against the rules is refused with the reason, and in a frame a person set it becomes a proposal", "向容器加一张卡片；不合规则的写入被拒绝并说明原因，人定的容器里转为提案"),
    ("frame.update", "Change a card's title, body or fields; what a person set becomes a suggestion", "修改卡片的标题、正文或字段；人设定过的内容只能提建议"),
    ("frame.move", "Move a card to another of the frame's states; a done state may need a person's confirmation", "把卡片移到容器的另一个状态；进入完成类状态可能需要人确认"),
    ("frame.link", "Link a card to another card, in this frame or another; the source frame's rules must allow lines", "把卡片连到另一张卡片，可以跨容器；起点容器的规则须允许连线"),
    ("frame.archive", "Archive a card, for example when a lane is over its limit", "归档一张卡片，例如某一列超过上限时"),
    ("frame.propose", "Propose a rule change; only a person applies it, in the App", "提出改规则的提案；只由人在 App 中采纳"),
    ("snapshot", "Write a recoverable snapshot through the single main writer", "由唯一主干写入器生成可恢复快照"),
    ("hub", "Request a hub rebuild; it must then be confirmed in the App", "请求重建 hub，必须随后在 App 中明确确认"),
    ("hub.rebuild", "Request a hub rebuild from the snapshot; the person confirms it in the App", "请求从快照重建 hub；由人在 App 中确认"),
    ("daemon", "Manage this user's daemon (JSON)", "管理本用户守护进程（JSON）"),
    ("daemon.ensure", "Start the daemon if needed and print its address", "确保守护进程在运行，并输出它的地址"),
    ("daemon.status", "Show whether the daemon is running", "查看守护进程是否在运行"),
    ("daemon.stop", "Stop the daemon", "停止守护进程"),
    ("init", "Initialize: project ID, .towermd/ config, AGENTS.md and a docs skeleton (nothing is committed)", "初始化：生成项目 ID、.towermd/ 配置与 AGENTS.md、文档骨架（不提交）"),
    ("dev", "Development kit: cold-start scan, baseline drafts and completion verification", "开发套件：冷启动扫描、基线草稿与完成验证"),
    ("dev.check", "Check the current task's paths, slow-layer protection and main gates", "检查当前任务的路径、慢层保护和主干门禁"),
    ("dev.finish", "Freeze the commit and run the real verification in the background; a self-reported done is not progress", "冻结提交并在后台运行真实验证；不把自述完成算作进度"),
    ("dev.clarify", "Restate a task waiting for clarification, draft its done-when and ask at most 3 questions; only the person in the App finalizes the card", "复述待澄清的任务、起草完成标准并提出不超过 3 个问题；委托卡只由 App 中的人定稿"),
    ("dev.trailers", "Print the commit trailers once G5 passes; the final evidence is stored durably first", "通过 G5 后输出提交 trailer；终态证据先耐久保存"),
    ("dev.scan", "Cold-start scan: really runs the repository's verification commands (inherited environment, network allowed); never approves a baseline", "冷启动扫描：真实运行仓库验证命令（继承环境、允许网络）；不批准基线"),
    ("dev.scan.start", "Start a scan; retrying with the same request ID returns the same scan", "启动扫描；同一请求 ID 重试返回同一扫描"),
    ("dev.scan.wait", "Wait for a scan, at most 90 seconds per call; exit code 5 while it still runs", "等待扫描，每次最多 90 秒；仍在运行时退出码为 5"),
    ("dev.scan.show", "Read a scan report", "读取扫描报告"),
    ("dev.scan.cancel", "Cancel a scan; returns only after the verification processes and worktrees are cleaned up", "取消扫描；等待验证进程和工作树清理完毕后才结束"),
    ("dev.draft", "Baseline synthesis drafts; only the App approves", "基线综合草稿；只有 App 能批准"),
    ("dev.draft.template", "Export the synthesis template from a finished scan; its content is not synthesized by an agent yet", "从已完成的扫描导出综合模板；内容尚未由 Agent 综合"),
    ("dev.draft.submit", "Submit S6 JSON as an unapproved draft; pushes a private draft ref when origin exists", "提交 S6 JSON 为未批准草稿；有 origin 时推送私有草稿引用"),
    ("dev.draft.show", "Read a synthesis draft", "读取综合草稿"),
    ("dev.draft.ls", "List this project's durable synthesis drafts", "列出本项目的耐久综合草稿"),
    ("dev.draft.compare", "Compare a draft started from the approved baseline with that baseline: added, removed, renamed, moved, merged, split, and the impact a proposal would carry", "对比从已批准基线开始的草稿与该基线：新增、删除、改名、移动、合并、拆分，以及提案将带的影响"),
    ("schema", "Print the JSON Schema of a command", "输出命令的 JSON Schema"),
];
/// (command path, argument id, en, zh); global options live on "".
const ARGS: &[(&str, &str, &str, &str)] = &[
    ("", "run", "Name the local run explicitly, overriding the worktree and branch lookup", "显式指定本机运行，覆盖工作树与分支自动定位"),
    ("", "json", "Print the JSON envelope (default when stdout is not a terminal)", "输出 JSON 信封（stdout 不是终端时的默认）"),
    ("", "text", "Print text for people (default when stdout is a terminal)", "输出给人看的文本（stdout 是终端时的默认）"),
    ("", "quiet", "In text mode print only messages and errors", "文本模式下只输出消息与错误"),
    ("", "lang", "Language of local text; when omitted, TOWERMD_LANG, LC_ALL, LANG and the system language are tried in turn", "本地文字的语言；省略时依次看 TOWERMD_LANG、LC_ALL、LANG 与系统语言"),
    ("", "request_id", "Request ID of a write command (UUID v7); generated when omitted, reuse it to retry safely after a failure", "写命令的请求 ID（UUID v7）；省略时自动生成，失败后沿用它可安全重试"),
    ("", "project", "Project path or ID; an ID must be the current repository's or one this user ran towermd in", "项目路径或项目 ID；ID 须是当前仓库的，或本用户运行过 towermd 的项目"),
    ("show", "id", "Any object ID; the project canvas when omitted or the project's own ID", "任意对象 ID；省略或为项目 ID 时显示项目画布"),
    ("needs", "all", "Include the items waiting under later and the quiet ones", "包括“稍后”中的条目和安静对象的条目"),
    ("since", "from", "Start time: 30m, 12h, 2d, a date or an RFC 3339 time; your last look by default", "起始时间：30m、12h、2d、日期或 RFC 3339 时间；默认为上次查看"),
    ("ls", "kind", "Kind of object", "对象种类"),
    ("ls", "state", "Only rows in this state, e.g. ready, open, accepted", "只列此状态的行，如 ready、open、accepted"),
    ("ls", "since", "Only rows at or after this time; rows that keep no time are left out", "只列此时间及以后的行；没有时间记录的行不列出"),
    ("history", "id", "Object ID", "对象 ID"),
    ("history", "since", "Only events at or after this time", "只列此时间及以后的事件"),
    ("watch", "scope", "Only events about this object", "只输出关于此对象的事件"),
    ("open", "id", "Object to locate later; the App opens on the project for now", "要定位的对象；目前 App 只打开到项目"),
    ("decision.show", "id", "Decision ID", "决策 ID"),
    ("completion", "shell", "Shell to complete for", "目标 shell"),
    ("task.add", "input", "JSON file; - means standard input", "JSON 文件；- 表示标准输入"),
    ("task.edit", "input", "JSON file with the whole task, ID included; - means standard input", "包含完整任务（含 ID）的 JSON 文件；- 表示标准输入"),
    ("task.ls", "state", "Only tasks in this state: proposed, ready, running, paused, verifying, held, merged, needs_review", "只列此状态的任务：proposed、ready、running、paused、verifying、held、merged、needs_review"),
    ("task.claim", "id", "Task ID; when omitted, claims the only claimable task or lists the candidates", "任务 ID；省略时领取唯一可领取的任务，否则列出候选"),
    ("task.claim", "harness", "Self-reported name of the agent tool (e.g. claude-code), not a trusted identity; needs --harness-version", "自报的受托者工具名（如 claude-code），不代表可信身份；须同时给 --harness-version"),
    ("task.claim", "harness_version", "Self-reported tool release; needs --harness", "自报的工具版本号；须同时给 --harness"),
    ("task.claim", "vendor", "Self-reported vendor of the model; needs --harness", "自报的模型厂商；须同时给 --harness"),
    ("task.claim", "model", "Self-reported model; needs --harness", "自报的模型；须同时给 --harness"),
    ("task.claim", "skills_version", "Self-reported skill pack release; needs --harness", "自报的技能包版本号；须同时给 --harness"),
    ("task.show", "id", "Task ID; the current run's task when omitted", "任务 ID；省略时为当前运行的任务"),
    ("propose", "input", "JSON file; - means standard input", "JSON 文件；- 表示标准输入"),
    ("pause", "scope", "Scope; C0 only supports all", "范围；C0 只支持 all"),
    ("resume", "scope", "Scope; C0 only supports all", "范围；C0 只支持 all"),
    ("resume", "pause_event", "Reuse the original pause event ID when retrying exactly; the current pause is read when omitted", "精确重试时沿用原暂停事件 ID；省略时读取当前暂停"),
    ("ask", "body", "The question", "问题"),
    ("ask", "options", "An answer option; repeat for several", "答案选项；可重复"),
    ("ask", "recommended", "Zero-based index of the recommended option, only a suggestion", "推荐选项的零起始下标，仅为建议"),
    ("ask", "context", "Repository path that gives context; repeat for several", "提供上下文的仓库路径；可重复"),
    ("ask", "target", "ID of an object the question affects; repeat for several", "问题影响的对象 ID；可重复"),
    ("ask", "parked", "Park the question instead of waiting for a live answer", "挂起问题，不等待实时回答"),
    ("wait", "hold", "Question ID returned by ask (H-…), or verification ID returned by dev finish (E-…)", "ask 返回的问题 ID（H-…），或 dev finish 返回的验证 ID（E-…）"),
    ("wait", "seconds", "How long to wait this call, 1 to 85 seconds; 85 by default", "本次等待的秒数，1 到 85；默认 85"),
    ("idea.add", "body", "The idea in one line, at most 200 characters", "一句话的想法，不换行，不超过 200 字"),
    ("ext.show", "id", "Extension ID, the directory name under .towermd/extensions/", "扩展 ID，即 .towermd/extensions/ 下的目录名"),
    ("frame.show", "frame", "Frame ID, such as C-reading", "容器 ID，例如 C-reading"),
    ("frame.add", "frame", "Frame ID, such as C-reading", "容器 ID，例如 C-reading"),
    ("frame.add", "title", "Card title, one line within the frame's limit; longer is refused, never cut", "卡片标题，一行且不超过容器上限；超长直接拒绝，不截断"),
    ("frame.add", "body", "Card body within the frame's limit", "卡片正文，不超过容器上限"),
    ("frame.add", "state", "One of the frame's states; defaults to the first", "容器的状态之一；默认第一个"),
    ("frame.add", "fields", "A field the rules allow, as KEY=VALUE; repeat for several", "规则允许的字段，写成 KEY=VALUE；可重复"),
    ("frame.add", "parent", "Card this one sits under, for nesting and trees", "这张卡片挂在哪张卡片下，用于嵌套和树"),
    ("frame.update", "card", "Card ID, such as C-reading.3", "卡片 ID，例如 C-reading.3"),
    ("frame.update", "title", "New title", "新标题"),
    ("frame.update", "body", "New body", "新正文"),
    ("frame.update", "fields", "A field to set, as KEY=VALUE; repeat for several", "要设的字段，写成 KEY=VALUE；可重复"),
    ("frame.update", "clear", "A field to clear; repeat for several", "要清除的字段；可重复"),
    ("frame.move", "card", "Card ID, such as C-reading.3", "卡片 ID，例如 C-reading.3"),
    ("frame.move", "state", "The state to move to; the rules' transitions apply", "目标状态；受规则中的流转约束"),
    ("frame.link", "card", "Card ID the link starts from", "连线起点的卡片 ID"),
    ("frame.link", "to", "Card ID the link points to, in any frame", "连线终点的卡片 ID，可在任一容器"),
    ("frame.archive", "card", "Card ID, such as C-reading.3", "卡片 ID，例如 C-reading.3"),
    ("frame.propose", "frame", "Frame ID, such as C-reading", "容器 ID，例如 C-reading"),
    ("frame.propose", "rules", "JSON file with the rules to change, such as {\"per_lane_max\": 8}; - reads stdin", "要改的规则 JSON 文件，例如 {\"per_lane_max\": 8}；- 表示读标准输入"),
    ("frame.add", "from", "Self-reported sender ID, not a trusted identity; defaults to the run's delegate and is required outside a run; it holds the frame's lease while writing", "自报的发送者 ID，不代表可信身份；默认是当前运行的受托者，运行之外必填；写入期间持有容器租约"),
    ("frame.add", "from_kind", "delegate (person, agent, automation) or source (check, connector); default source", "delegate（人、Agent、自动化）或 source（检查、连接器）；默认 source"),
    ("frame.update", "from", "Self-reported sender ID, not a trusted identity; defaults to the run's delegate and is required outside a run; it holds the frame's lease while writing", "自报的发送者 ID，不代表可信身份；默认是当前运行的受托者，运行之外必填；写入期间持有容器租约"),
    ("frame.update", "from_kind", "delegate (person, agent, automation) or source (check, connector); default source", "delegate（人、Agent、自动化）或 source（检查、连接器）；默认 source"),
    ("frame.move", "from", "Self-reported sender ID, not a trusted identity; defaults to the run's delegate and is required outside a run; it holds the frame's lease while writing", "自报的发送者 ID，不代表可信身份；默认是当前运行的受托者，运行之外必填；写入期间持有容器租约"),
    ("frame.move", "from_kind", "delegate (person, agent, automation) or source (check, connector); default source", "delegate（人、Agent、自动化）或 source（检查、连接器）；默认 source"),
    ("frame.link", "from", "Self-reported sender ID, not a trusted identity; defaults to the run's delegate and is required outside a run; it holds the frame's lease while writing", "自报的发送者 ID，不代表可信身份；默认是当前运行的受托者，运行之外必填；写入期间持有容器租约"),
    ("frame.link", "from_kind", "delegate (person, agent, automation) or source (check, connector); default source", "delegate（人、Agent、自动化）或 source（检查、连接器）；默认 source"),
    ("frame.archive", "from", "Self-reported sender ID, not a trusted identity; defaults to the run's delegate and is required outside a run; it holds the frame's lease while writing", "自报的发送者 ID，不代表可信身份；默认是当前运行的受托者，运行之外必填；写入期间持有容器租约"),
    ("frame.archive", "from_kind", "delegate (person, agent, automation) or source (check, connector); default source", "delegate（人、Agent、自动化）或 source（检查、连接器）；默认 source"),
    ("frame.propose", "from", "Self-reported sender ID, not a trusted identity; defaults to the run's delegate and is required outside a run; it holds the frame's lease while writing", "自报的发送者 ID，不代表可信身份；默认是当前运行的受托者，运行之外必填；写入期间持有容器租约"),
    ("frame.propose", "from_kind", "delegate (person, agent, automation) or source (check, connector); default source", "delegate（人、Agent、自动化）或 source（检查、连接器）；默认 source"),
    ("answer", "hold", "Question ID returned by ask", "ask 返回的问题 ID"),
    ("answer", "body", "The person's answer, as they said it", "人的回答，按原话转述"),
    ("answer", "relayed", "Confirm the person gave this answer in the current session; it is only relayed, never recorded as a decision, a rule or an approval", "确认这是人在当前会话中亲口给出的回答；只做转述，不记为决策、规则或批准"),
    ("answer", "harness", "Self-reported name of the agent tool (e.g. claude-code), not a trusted identity; needs --harness-version", "自报的受托者工具名（如 claude-code），不代表可信身份；须同时给 --harness-version"),
    ("answer", "harness_version", "Self-reported tool release; needs --harness", "自报的工具版本号；须同时给 --harness"),
    ("handled", "prompt", "Prompt ID from delivered[].message.id, such as P-...", "delivered[].message.id 中的插话 ID，例如 P-..."),
    ("handled", "note", "What you did about it, one line of at most 200 characters", "你做了什么，一行，不超过 200 字"),
    ("reply", "request", "Request ID from delivered[]", "delivered[] 中的请求 ID"),
    ("reply", "state", "Where the work stands", "工作所处状态"),
    ("reply", "summary", "At most 100 characters", "不超过 100 字"),
    ("reply", "next", "What comes next", "下一步"),
    ("reply", "refs", "Evidence such as a file, commit or URL; a node question needs at least one; repeat for several", "依据，如文件、提交或网址；回答节点提问至少一条；可重复"),
    ("reply", "from", "Self-reported sender ID outside a run; must be a recipient of the request; not for checkpoints", "运行之外自报的发送者 ID，须是该请求的收件人；检查点汇报不可用"),
    ("report", "state", "Where the work stands; there is no done, only claimed_done", "工作所处状态；没有 done，只有 claimed_done（自称完成）"),
    ("report", "summary", "At most 100 characters; longer is refused, never cut", "不超过 100 字；超长直接拒绝，不截断"),
    ("report", "next", "What comes next", "下一步"),
    ("report", "progress", "DONE/TOTAL, e.g. 3/7", "已完成/总数，如 3/7"),
    ("report", "unit", "Unit of --progress, e.g. files", "--progress 的单位，如 files"),
    ("report", "answers", "Declare that this sender answers questions on the target; stays until --no-answers", "声明该发送者回答此对象上的提问；直到 --no-answers 为止"),
    ("report", "no_answers", "Stop answering questions on the target", "不再回答此对象上的提问"),
    ("report", "target", "ID of the object it is about; defaults to the run's task, else the project", "所涉对象的 ID；默认是当前运行的任务，否则是项目"),
    ("report", "from", "Self-reported sender ID, not a trusted identity; defaults to the run's delegate and is required outside a run", "自报的发送者 ID，不代表可信身份；默认是当前运行的受托者，运行之外必填"),
    ("report", "from_kind", "delegate (person, agent, automation) or source (check, connector); default source", "delegate（人、Agent、自动化）或 source（检查、连接器）；默认 source"),
    ("report", "refs", "Reference such as a file, commit or URL; repeat for several", "引用，如文件、提交或网址；可重复"),
    ("signal", "kind", "Namespaced kind, e.g. eng.crap; core.* is reserved for Tower.md", "带命名空间的种类，如 eng.crap；core.* 留给 Tower.md"),
    ("signal", "level", "log (timeline only), passive (≡ digest), active (○ next view), time_sensitive (‼ immediate; only the App notifies)", "log（只进时间线）、passive（≡ 摘要）、active（○ 下次查看）、time_sensitive（‼ 立即，只由 App 发通知）"),
    ("signal", "title", "At most 30 characters; longer is refused, never cut", "不超过 30 字；超长直接拒绝，不截断"),
    ("signal", "detail", "At most 100 characters", "不超过 100 字"),
    ("signal", "value", "Measured value; numbers and true/false are kept as such", "测得的值；数字和 true/false 按原类型保存"),
    ("signal", "threshold", "The threshold the value is compared with", "与值比较的阈值"),
    ("signal", "deterministic", "Declare the check deterministic; whether it gates anything is the person's call", "声明该检查是确定性的；是否据此拦截由人决定"),
    ("signal", "target", "ID of the object it is about; defaults to the run's task, else the project", "所涉对象的 ID；默认是当前运行的任务，否则是项目"),
    ("signal", "from", "Self-reported sender ID, not a trusted identity; defaults to the run's delegate and is required outside a run", "自报的发送者 ID，不代表可信身份；默认是当前运行的受托者，运行之外必填"),
    ("signal", "from_kind", "delegate (person, agent, automation) or source (check, connector); default source", "delegate（人、Agent、自动化）或 source（检查、连接器）；默认 source"),
    ("signal", "refs", "Reference such as a file, commit or URL; repeat for several", "引用，如文件、提交或网址；可重复"),
    ("init", "path", "Any path inside the repository; the current directory by default", "仓库内任意路径，默认当前目录"),
    ("init", "docs_dir", "User docs directory, relative to the repository root; required for a new config when not interactive", "用户文档目录，相对仓库根；非交互新配置必须显式指定"),
    ("dev.finish", "wait", "Wait for an existing verification, at most 90 seconds per call", "等待已有验证，每次最多 90 秒"),
    ("dev.finish", "claims", "JSON file [{\"criterion\": 0, \"note\": \"…\"}] with your account per done-when criterion; recorded as self-report, never counted as proof", "JSON 文件 [{\"criterion\": 0, \"note\": \"…\"}]，逐条写完成标准的自述；只记为自述，不算证据"),
    ("dev.clarify", "task", "Task ID", "任务 ID"),
    ("dev.clarify", "input", "JSON file {\"restated\", \"done_when\": [], \"questions\": []}; - means standard input", "JSON 文件 {\"restated\", \"done_when\": [], \"questions\": []}；- 表示标准输入"),
    ("dev.clarify", "harness", "Self-reported harness name, not a trusted identity; needs --harness-version", "自报的 harness 名称，不代表可信身份；须同时给 --harness-version"),
    ("dev.clarify", "harness_version", "Self-reported harness version; needs --harness", "自报的 harness 版本；须同时给 --harness"),
    ("dev.clarify", "vendor", "Self-reported vendor of the model; needs --harness", "自报的模型厂商；须同时给 --harness"),
    ("dev.clarify", "model", "Self-reported model; needs --harness", "自报的模型；须同时给 --harness"),
    ("dev.clarify", "skills_version", "Self-reported skill pack version; needs --harness", "自报的技能包版本；须同时给 --harness"),
    ("dev.trailers", "task", "Task ID", "任务 ID"),
    ("dev.scan.start", "from_baseline", "Start from the current approved baseline: keep its modules, commands and layout instead of clustering again; needs an approved baseline", "从当前已批准基线开始：沿用其模块、验证命令与布局，不重新聚类；须已有批准基线"),
    ("dev.scan.wait", "id", "Scan ID", "扫描 ID"),
    ("dev.scan.show", "id", "Scan ID", "扫描 ID"),
    ("dev.scan.cancel", "id", "Scan ID", "扫描 ID"),
    ("dev.draft.template", "scan", "ID of a finished scan", "已完成扫描的 ID"),
    ("dev.draft.submit", "input", "JSON file; - means standard input", "JSON 文件；- 表示标准输入"),
    ("dev.draft.submit", "producer", "Self-reported <producer>/<version> source, not a trusted identity", "自报 <producer>/<version> 来源，不代表可信身份"),
    ("dev.draft.show", "id", "Draft ID", "草稿 ID"),
    ("dev.draft.compare", "id", "Draft ID", "草稿 ID"),
    ("schema", "name", "Dotted command name, e.g. dev.draft.submit", "点分命令名，例如 dev.draft.submit"),
];
/// (leaf path, examples, one per line)
const EXAMPLES: &[(&str, &str)] = &[
    ("up", "towermd up"),
    ("show", "towermd show\ntowermd show T-1\ntowermd --json show H-1"),
    ("needs", "towermd needs\ntowermd needs --all"),
    ("since", "towermd since\ntowermd since --from 2d\ntowermd since --from 2026-10-20"),
    ("overview", "towermd overview"),
    ("ls", "towermd ls tasks --state ready\ntowermd ls proposals --since 12h\ntowermd ls decisions"),
    ("history", "towermd history T-1\ntowermd history T-1 --since 2d"),
    ("watch", "towermd watch\ntowermd --json watch --scope T-1"),
    ("open", "towermd open\ntowermd open T-1"),
    ("decision.show", "towermd decision show D-1"),
    ("decision.ls", "towermd decision ls"),
    ("rule.ls", "towermd rule ls"),
    ("rebuild-index", "towermd rebuild-index"),
    ("doctor", "towermd doctor"),
    ("whoami", "towermd whoami\ntowermd --run R-1 whoami"),
    ("completion", "towermd completion zsh\ntowermd completion bash"),
    ("task.edit", "towermd task edit task.json\ntowermd task edit -"),
    ("task.ls", "towermd task ls\ntowermd task ls --state ready"),
    ("task.add", "towermd task add task.json\ntowermd task add -"),
    ("task.claim", "towermd task claim\ntowermd task claim T-1\ntowermd task claim T-1 --harness claude-code --harness-version 2.1 --vendor anthropic --model claude-opus --skills-version 0.2"),
    ("task.show", "towermd task show\ntowermd task show T-1"),
    ("task.release", "towermd task release"),
    ("propose", "towermd propose proposal.json\ntowermd propose -"),
    ("pause", "towermd pause all"),
    ("resume", "towermd resume all"),
    ("status", "towermd status\ntowermd --text status"),
    ("ask", "towermd ask \"Which index?\" --option \"B-tree\" --option \"Hash\" --recommended 0\ntowermd ask \"Which index?\" --option \"B-tree\" --option \"Hash\" --context src/index.rs\ntowermd ask \"Ship Friday?\" --parked"),
    ("wait", "towermd wait H-1\ntowermd wait H-1 --seconds 30\ntowermd wait E-1"),
    ("answer", "towermd answer H-1 \"Use B-tree\" --relayed"),
    ("handled", "towermd handled P-0190d2c3-0000-7000-8000-000000000001 --note \"Reverted the dependency bump\""),
    ("reply", "towermd reply R-R-1-1 --state in_progress --summary \"Parser done, tests next\" --next \"Write tests\"\ntowermd reply <request-id> --state in_progress --summary \"Regex backtracking\" --ref src/parser.rs"),
    ("report", "towermd report --state in_progress --summary \"Parser done, tests next\" --progress 3/7 --unit files\ntowermd report --state blocked --summary \"Waiting for API keys\" --from ci/nightly --target T-1"),
    ("signal", "towermd signal eng.crap --level active --title \"CRAP above 30\" --value 42 --threshold 30 --ref src/parser.rs\ntowermd signal ops.disk --level passive --title \"Disk 85% full\" --from cron/disk --from-kind source"),
    ("idea.add", "towermd idea add \"Export the weekly report as CSV\""),
    ("ext.ls", "towermd ext ls"),
    ("ext.show", "towermd ext show eng-clean"),
    ("template.ls", "towermd template ls"),
    ("frame.show", "towermd frame show C-reading"),
    ("frame.add", "towermd frame add C-reading --title \"Read the parser\" --state todo --field owner=bruce"),
    ("frame.update", "towermd frame update C-reading.3 --body \"Half done\" --clear owner"),
    ("frame.move", "towermd frame move C-reading.3 doing"),
    ("frame.link", "towermd frame link C-reading.3 C-reading.1"),
    ("frame.archive", "towermd frame archive C-reading.3"),
    ("frame.propose", "towermd frame propose C-reading rules.json"),
    ("snapshot", "towermd snapshot"),
    ("hub.rebuild", "towermd hub rebuild"),
    ("daemon.ensure", "towermd daemon ensure"),
    ("daemon.status", "towermd daemon status"),
    ("daemon.stop", "towermd daemon stop"),
    ("init", "towermd init --docs-dir docs\ntowermd init path/to/repo --docs-dir docs"),
    ("dev.check", "towermd dev check"),
    ("dev.finish", "towermd dev finish\ntowermd dev finish --claims claims.json\ntowermd dev finish --wait E-1"),
    ("dev.clarify", "towermd dev clarify T-1 clarify.json\ntowermd dev clarify T-1 - --harness claude-code --harness-version 2.1"),
    ("dev.trailers", "towermd dev trailers T-1"),
    ("dev.scan.start", "towermd dev scan start\ntowermd dev scan start --from-baseline"),
    ("dev.scan.wait", "towermd dev scan wait S-1"),
    ("dev.scan.show", "towermd dev scan show S-1"),
    ("dev.scan.cancel", "towermd dev scan cancel S-1"),
    ("dev.draft.template", "towermd dev draft template S-1"),
    ("dev.draft.submit", "towermd dev draft submit draft.json --producer claude-code/2.1\ntowermd dev draft submit - --producer codex/0.5"),
    ("dev.draft.show", "towermd dev draft show d-1"),
    ("dev.draft.ls", "towermd dev draft ls"),
    ("dev.draft.compare", "towermd dev draft compare d-1"),
    ("schema", "towermd schema\ntowermd schema dev.draft.submit"),
];

const EXIT_CODES_EN: &str = "Exit codes:\n  0 ok\n  1 internal\n  2 usage, too_long\n  3 not_found, not_initialized\n  4 lease_busy, stale_epoch, conflict\n  5 pending, deferred (no result yet: follow `next` to wait again; without `next` (deferred) end the turn, do not busy-wait)\n  6 guard_blocked, gate_failed, needs_proposal, rejected\n  7 needs_human\n  8 daemon_unavailable, hub_offline";
const EXIT_CODES_ZH: &str = "退出码：\n  0 成功\n  1 internal\n  2 usage、too_long\n  3 not_found、not_initialized\n  4 lease_busy、stale_epoch、conflict\n  5 pending、deferred（暂时没有结果：有 `next` 就按 `next` 继续等；没有 `next`（deferred）就结束回合，不要忙等）\n  6 guard_blocked、gate_failed、needs_proposal、rejected\n  7 needs_human\n  8 daemon_unavailable、hub_offline";

/// FR-199: the root help lists commands in the spec's §2.1–2.7 groups; the
/// development kit's own commands appear only under `towermd help dev`.
const GROUPS: &[(&str, &str, &[&str])] = &[
    ("Look", "看", &["status", "needs", "since", "overview", "show", "ls", "history", "watch", "open"]),
    ("Tower API verbs", "接入接口", &["report", "signal", "ask", "wait", "reply", "handled", "propose", "frame"]),
    ("Person's actions (approved in the App; the command line only relays or requests)", "人的动作（在 App 中批准；命令行只转述或请求）", &["answer", "pause", "resume", "hub"]),
    ("Plan objects", "计划对象", &["task", "idea", "decision", "rule"]),
    ("Extensions", "扩展", &["ext", "template"]),
    ("Infrastructure", "基础设施与元命令", &["init", "up", "daemon", "snapshot", "rebuild-index", "doctor", "whoami", "schema", "completion", "help"]),
    ("Development kit", "开发套件", &["dev"]),
];

pub fn localized(command: Command, lang: Lang) -> Command {
    let command = apply(command, "", lang).after_help(pick_in(lang, EXIT_CODES_EN, EXIT_CODES_ZH));
    let template = format!(
        "{{about-with-newline}}\n{{usage-heading}} {{usage}}\n\n{}{}\n{{options}}{{after-help}}",
        grouped(&command, lang),
        pick_in(lang, "Options:", "选项："),
    );
    command.help_template(template)
}

fn grouped(command: &Command, lang: Lang) -> String {
    let mut text = String::new();
    for (en, zh, names) in GROUPS {
        text.push_str(&format!("{}\n", pick_in(lang, *en, *zh)));
        for name in *names {
            let about = match command.find_subcommand(name) {
                Some(sub) => sub.get_about().map(ToString::to_string).unwrap_or_default(),
                None => pick_in(lang, "Print help for a command", "显示命令帮助").to_owned(),
            };
            text.push_str(&format!("  {name:<14} {about}\n"));
        }
        text.push('\n');
    }
    text
}

fn apply(mut command: Command, path: &str, lang: Lang) -> Command {
    if let Some((_, en, zh)) = ABOUT.iter().find(|(p, _, _)| *p == path) {
        command = command.about(pick_in(lang, *en, *zh));
    }
    for (_, id, en, zh) in ARGS.iter().filter(|(p, _, _, _)| *p == path) {
        let text = pick_in(lang, *en, *zh);
        command = command.mut_arg(*id, |arg| arg.help(text));
    }
    if let Some((_, lines)) = EXAMPLES.iter().find(|(p, _)| *p == path) {
        let title = pick_in(lang, "Examples:", "示例：");
        let body: String = lines.lines().map(|line| format!("\n  {line}")).collect();
        command = command.after_help(format!("{title}{body}"));
    }
    let names: Vec<String> = command.get_subcommands().map(|sub| sub.get_name().to_owned()).collect();
    for name in names {
        let child = if path.is_empty() { name.clone() } else { format!("{path}.{name}") };
        command = command.mut_subcommand(name, |sub| apply(sub, &child, lang));
    }
    command
}

/// Splits a documented command line; only double quotes group words.
#[cfg(test)]
pub fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut started = false;
    for ch in line.chars() {
        match ch {
            '"' => { quoted = !quoted; started = true; }
            ch if ch.is_whitespace() && !quoted => {
                if started { words.push(std::mem::take(&mut word)); started = false; }
            }
            ch => { word.push(ch); started = true; }
        }
    }
    if started { words.push(word); }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::Cli;
    use clap::{CommandFactory, Parser};

    fn visit(command: &Command, path: &str, out: &mut Vec<(String, Vec<String>, bool)>) {
        let args = command.get_arguments().filter(|arg| !arg.is_hide_set() && !matches!(arg.get_id().as_str(), "help" | "version"))
            .filter(|arg| !arg.is_global_set() || path.is_empty())
            .map(|arg| arg.get_id().to_string()).collect();
        out.push((path.to_owned(), args, !command.has_subcommands()));
        for sub in command.get_subcommands().filter(|sub| !sub.is_hide_set() && sub.get_name() != "help") {
            let child = if path.is_empty() { sub.get_name().to_owned() } else { format!("{path}.{}", sub.get_name()) };
            visit(sub, &child, out);
        }
    }

    #[test]
    fn every_visible_command_and_argument_is_described_in_both_languages() {
        let mut nodes = Vec::new();
        visit(&Cli::command(), "", &mut nodes);
        for (path, args, leaf) in nodes {
            assert!(ABOUT.iter().any(|(p, en, zh)| *p == path && !en.is_empty() && !zh.is_empty()), "about: {path:?}");
            for arg in args {
                assert!(ARGS.iter().any(|(p, id, _, _)| *p == path && *id == arg), "arg: {path:?} {arg}");
            }
            if leaf {
                assert!(EXAMPLES.iter().any(|(p, _)| *p == path), "examples: {path}");
            }
        }
        let help = localized(Cli::command(), Lang::ZhCn).render_long_help().to_string();
        assert!(help.contains("rejected") && help.contains("退出码"), "{help}");
    }

    #[test]
    fn root_help_groups_every_command_once_and_hides_the_kit() {
        let root = Cli::command();
        let visible: Vec<String> = root.get_subcommands().filter(|sub| !sub.is_hide_set()).map(|sub| sub.get_name().to_owned()).collect();
        for name in &visible {
            let count = GROUPS.iter().filter(|(_, _, names)| names.contains(&name.as_str())).count();
            assert_eq!(count, 1, "{name} must be in exactly one group");
        }
        for (_, _, names) in GROUPS {
            for name in *names {
                assert!(*name == "help" || visible.iter().any(|known| known == name), "unknown command {name} in a group");
            }
        }
        for lang in [Lang::En, Lang::ZhCn] {
            let help = localized(Cli::command(), lang).render_help().to_string();
            assert!(help.contains("  needs ") && help.contains("  dev "), "{help}");
            for leaf in ["scan", "draft", "trailers", "clarify", "finish", "check"] {
                assert!(!help.lines().any(|line| line.trim_start().starts_with(leaf)), "{leaf} leaked into the root help:\n{help}");
            }
            let mut root = localized(Cli::command(), lang);
            let dev = root.find_subcommand_mut("dev").unwrap().render_help().to_string();
            assert!(dev.contains("scan") && dev.contains("finish"), "{dev}");
        }
    }

    #[test]
    fn every_example_parses() {
        for (path, lines) in EXAMPLES {
            for line in lines.lines() {
                let words = shell_words(line);
                assert_eq!(words.first().map(String::as_str), Some("towermd"), "{path}: {line}");
                if let Err(error) = Cli::try_parse_from(&words) {
                    panic!("{path}: {line}: {error}");
                }
            }
        }
    }
}
