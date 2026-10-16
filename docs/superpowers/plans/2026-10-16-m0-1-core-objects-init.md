---
type: Plan
title: M0-1 核心对象与 init 实现计划
description: Tower.md M0 第一份实现计划：Rust workspace 骨架、OKF v0.2 文档往返、对象 type 与层级登记、`.towermd/config.yaml`、`towermd init`（项目 ID、用户文档目录、`.towermd/AGENTS.md`）。
tags: [towermd, plan, M0, OKF, init]
generated: { by: claude-code/opus-5.5, at: 2026-10-16T16:00:00Z }
sources:
  - id: spec
    resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
    title: Tower.md 规格 v0.5（FR-01、FR-07、FR-81、FR-82、FR-98，6.1–6.2 节，18 节 M0）
status: draft
---

# M0-1 核心对象与 init 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建起 Rust workspace，实现 OKF 文档的宽容读取与逐字节往返、对象 `type` 的唯一登记，以及不经守护进程、只写当前工作树的 `towermd init`。

**Architecture:** 一个与平台无关的核心 crate `towermd-core`（`okf`、`types`、`config`、`repo`、`init` 五个模块），一个只做参数解析和输出的 CLI crate `towermd`。init 是规格 FR-98 明文允许的唯一绕过守护进程的写入：只生成文件，不提交。

**Tech Stack:** Rust stable（edition 2021），serde_yaml 0.9，uuid 1（v7），clap 4（derive），tempfile 3（测试）。

**Spec:** `docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md`

M0 共四份计划，本计划是第一份：

| 计划 | 内容 |
| --- | --- |
| M0-1（本计划） | workspace、OKF 文档、type 登记、配置、`towermd init` |
| M0-2 | 本地 hub（定序、`eid` 幂等、租约与 `epoch`）、守护进程单实例、多终端运行判定、`status` 与 `task add/claim/show/release` |
| M0-3 | 快照写入、主干租约、`hub rebuild` |
| M0-4 | `tower.canvas` 编解码、冷启动 S1–S5 |

## Global Constraints

- **前置条件：本机须有 Rust 工具链。** 写本计划时机器上没有 `cargo` 和 `rustup`，计划里的代码**没有经过编译验证**，执行时以编译器和测试为准。装 rustup 会改动用户环境，须先经人同意，不得自行安装。
- 依赖版本沿用 Note.md（`~/git/note.md/src-tauri/Cargo.toml`）：`serde_yaml = "0.9"`、`uuid = { version = "1", features = ["v7"] }`、`tempfile = "3"`。clap 沿用 4，但加 `derive` 特性（Note.md 用的是 builder 写法）。这只是给已有依赖加特性，不算新增依赖，在 `tasks/todo.md` 记录理由即可。
- 测试里调用 git 时，一律设置 `GIT_CONFIG_GLOBAL=/dev/null` 和 `GIT_CONFIG_NOSYSTEM=1`，不读也不改用户的全局 git 配置。
- 测试里运行 `towermd` 二进制时，一律设置 `stdin(Stdio::null())`，否则 init 会在终端里等待交互输入。
- 提交只用显式路径（`git add <path>…`），不用 `git add .`。`AGENTS.md`、`CLAUDE.md`、`tasks/` 是用户未跟踪的文件，不纳入本计划的提交。不推送。
- 错误类型沿用 Note.md 的写法，用 `Result<T, String>`。面向人的提示文字用中文。
- 规格引用：FR-01（权限以 `layer` 为准）、FR-07（OKF）、FR-81（`.towermd/AGENTS.md`）、FR-82（type 唯一登记、宽容读取、往返保留）、FR-98（项目 ID、docs_dir、init 不提交）、16.1 节（`towermd init [path]`）。

## File map

| 文件 | 职责 |
| --- | --- |
| `Cargo.toml` | workspace 根 |
| `.gitignore` | 忽略 `/target` |
| `crates/towermd-core/Cargo.toml` | 核心 crate |
| `crates/towermd-core/src/lib.rs` | 导出模块 |
| `crates/towermd-core/src/okf.rs` | OKF 文档：解析、读写字段、往返 |
| `crates/towermd-core/src/types.rs` | `Layer`、`TYPES` 登记、`layer_of` |
| `crates/towermd-core/src/config.rs` | `.towermd/config.yaml` 读写，保留未知键 |
| `crates/towermd-core/src/repo.rs` | git 命令封装：`git`、`toplevel` |
| `crates/towermd-core/src/init.rs` | `init`、`normalize_docs_dir`、`render_agents_md` |
| `crates/towermd-core/templates/agents.md` | `.towermd/AGENTS.md` 模板 |
| `crates/towermd/Cargo.toml` | CLI crate，二进制名 `towermd` |
| `crates/towermd/src/main.rs` | clap 入口与 `init` 子命令 |
| `crates/towermd/tests/init_cli.rs` | CLI 集成测试 |

按目录批量加载对象（跳过 `tower.canvas`、`state/`、`.towermd/AGENTS.md` 等非对象文件，FR-07、FR-81）放在 M0-4，与冷启动一起做。

M0-2 起需要的 git 公共目录、当前分支、按 ref 读文件等辅助函数，到那时再加进 `repo.rs`，本计划不提前写。

---

### Task 1: workspace 骨架

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `crates/towermd-core/Cargo.toml`
- Create: `crates/towermd-core/src/lib.rs`
- Create: `crates/towermd/Cargo.toml`
- Create: `crates/towermd/src/main.rs`

- [ ] **Step 1: 确认工具链**

Run: `cargo --version && rustc --version`
Expected: 打印版本号。如果报 `command not found`，停下来告诉人，等人决定是否安装 rustup。

- [ ] **Step 2: 写 workspace 根与 .gitignore**

`Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/towermd-core", "crates/towermd"]

[workspace.package]
version = "0.0.0"
edition = "2021"
publish = false

[workspace.dependencies]
serde_yaml = "0.9"
uuid = { version = "1", features = ["v7"] }
clap = { version = "4", features = ["derive"] }
tempfile = "3"
towermd-core = { path = "crates/towermd-core" }
```

`.gitignore`:

```gitignore
/target
.DS_Store
```

- [ ] **Step 3: 写两个 crate 的清单与空入口**

`crates/towermd-core/Cargo.toml`:

```toml
[package]
name = "towermd-core"
version.workspace = true
edition.workspace = true
publish.workspace = true

[dependencies]
serde_yaml.workspace = true
uuid.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`crates/towermd-core/src/lib.rs`:

```rust
//! Tower.md 核心：对象模型、配置与初始化。守护进程、CLI、画布都只是它的适配层。
```

`crates/towermd/Cargo.toml`:

```toml
[package]
name = "towermd"
version.workspace = true
edition.workspace = true
publish.workspace = true

[[bin]]
name = "towermd"
path = "src/main.rs"

[dependencies]
clap.workspace = true
towermd-core.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`crates/towermd/src/main.rs`:

```rust
fn main() {}
```

- [ ] **Step 4: 构建**

Run: `cargo build`
Expected: `Finished`，生成 `Cargo.lock`。

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore crates/towermd-core/Cargo.toml crates/towermd-core/src/lib.rs crates/towermd/Cargo.toml crates/towermd/src/main.rs
git commit -m "Scaffold Rust workspace: towermd-core and towermd CLI"
```

---

### Task 2: OKF 文档往返（FR-07、FR-82）

**Files:**
- Create: `crates/towermd-core/src/okf.rs`
- Modify: `crates/towermd-core/src/lib.rs`

行为约定：
- 文件必须以 `---` 行开头，并有闭合的 `---` 行；frontmatter 必须是映射；`type` 必须是非空字符串。三者缺一则拒绝，这是 OKF 的硬性要求。
- 其余字段一律宽容：缺可选字段、未知 `type`、未知键都接受。
- 未修改时，`to_text()` 原样返回读入的字节，CRLF、注释、引号风格都不变。
- 修改后，frontmatter 用 serde_yaml 重新序列化：键序和未知键保留，注释丢失，正文原样保留。
- 写入时 `type` 必须是已登记的取值。这项检查放在 Task 3 的 `types::lookup` 里，本任务先留出 `Document::new` 和 `set` 这两个写入口，Task 3 再接上检查。

- [ ] **Step 1: 写失败的测试**

`crates/towermd-core/src/okf.rs`（先只写测试和空壳，让编译通过、测试失败）:

```rust
//! OKF v0.2 文档：Markdown 加 YAML frontmatter（FR-07）。
//!
//! 读取宽容、往返保真（FR-82）：未改动时 `to_text()` 原样返回读入的字节；
//! 改动后按 serde_yaml 重新序列化 frontmatter，键序与未知键保留，注释不保留。

use serde_yaml::{Mapping, Value};

#[derive(Debug, Clone)]
pub struct Document {
    front: Mapping,
    body: String,
    /// 读入的原文；任何字段被改动后置为 `None`，输出时改为重新序列化。
    original: Option<String>,
}

impl Document {
    pub fn parse(_text: &str) -> Result<Self, String> {
        Err("unimplemented".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULE: &str = "---\ntype: Module\nid: A-sync\nlayer: L1\n# 注释保留\nx-custom: { keep: [1, 2] }\npaths: ['src/sync/**']\n---\n负责同步。\n";

    #[test]
    fn unmodified_round_trip_is_byte_identical() {
        let doc = Document::parse(MODULE).unwrap();
        assert_eq!(doc.to_text(), MODULE);
    }

    #[test]
    fn crlf_round_trip_is_byte_identical() {
        let text = MODULE.replace('\n', "\r\n");
        let doc = Document::parse(&text).unwrap();
        assert_eq!(doc.to_text(), text);
    }

    #[test]
    fn reads_fields_and_body() {
        let doc = Document::parse(MODULE).unwrap();
        assert_eq!(doc.doc_type(), Some("Module"));
        assert_eq!(doc.get_str("id"), Some("A-sync"));
        assert_eq!(doc.body(), "负责同步。\n");
    }

    #[test]
    fn unknown_type_is_accepted() {
        let doc = Document::parse("---\ntype: FutureThing\n---\n").unwrap();
        assert_eq!(doc.doc_type(), Some("FutureThing"));
    }

    #[test]
    fn missing_or_empty_type_is_rejected() {
        assert!(Document::parse("---\nid: A\n---\n").is_err());
        assert!(Document::parse("---\ntype: ''\n---\n").is_err());
        assert!(Document::parse("---\n---\n").is_err());
    }

    #[test]
    fn missing_or_unclosed_frontmatter_is_rejected() {
        assert!(Document::parse("# 没有 frontmatter\n").is_err());
        assert!(Document::parse("---\ntype: Module\n").is_err());
        assert!(Document::parse("---\n- a\n- b\n---\n").is_err());
    }

    #[test]
    fn edit_keeps_unknown_keys_order_and_body() {
        let mut doc = Document::parse(MODULE).unwrap();
        doc.set("title", Value::from("同步")).unwrap();
        let out = doc.to_text();
        assert!(out.starts_with("---\ntype: Module\nid: A-sync\nlayer: L1\nx-custom:"), "{out}");
        assert!(out.contains("title: 同步\n"), "{out}");
        assert!(out.ends_with("---\n负责同步。\n"), "{out}");
        let again = Document::parse(&out).unwrap();
        assert_eq!(again.get("x-custom"), doc.get("x-custom"));
    }
}
```

在 `crates/towermd-core/src/lib.rs` 末尾加：

```rust
pub mod okf;
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p towermd-core okf`
Expected: 编译失败，报 `no method named to_text`、`doc_type`、`get_str`、`body`、`set`、`get`。

- [ ] **Step 3: 实现**

把 `okf.rs` 中 `impl Document` 整块替换为下面的代码，并在其后加入 `leading_frontmatter`（测试模块保持不变）:

```rust
impl Document {
    /// 读取 OKF 文档。只有三种情况拒绝：没有闭合的 frontmatter、
    /// frontmatter 不是映射、`type` 缺失或为空。
    pub fn parse(text: &str) -> Result<Self, String> {
        let (yaml, body) = leading_frontmatter(text)?;
        let front = match serde_yaml::from_str::<Value>(yaml)
            .map_err(|e| format!("frontmatter 不是合法 YAML：{e}"))?
        {
            Value::Mapping(m) => m,
            Value::Null => Mapping::new(),
            _ => return Err("frontmatter 必须是映射".into()),
        };
        let doc = Self { front, body: body.to_string(), original: Some(text.to_string()) };
        if doc.doc_type().is_none() {
            return Err("frontmatter 缺少非空的 `type`".into());
        }
        Ok(doc)
    }

    pub fn doc_type(&self) -> Option<&str> {
        self.get_str("type").filter(|t| !t.trim().is_empty())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.front.get(key)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    pub fn front(&self) -> &Mapping {
        &self.front
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    /// 写入一个 frontmatter 字段；已有键原位替换，新键追加在末尾。
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        self.front.insert(Value::from(key), value);
        self.original = None;
        Ok(())
    }

    pub fn to_text(&self) -> String {
        if let Some(original) = &self.original {
            return original.clone();
        }
        let yaml = serde_yaml::to_string(&self.front).expect("YAML 映射总能序列化");
        format!("---\n{yaml}---\n{}", self.body)
    }
}

/// 拆出首部 frontmatter，返回 `(yaml, body)`。分隔行允许 CRLF。
/// 写法同 Note.md 的 `plugin_runtime::cdr_managed_document::leading_frontmatter`。
fn leading_frontmatter(text: &str) -> Result<(&str, &str), String> {
    let mut offset = 0usize;
    let mut yaml_start = None;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let is_delim = line.trim_end_matches(['\r', '\n']) == "---";
        match yaml_start {
            None if is_delim => yaml_start = Some(offset),
            None => return Err("文档必须以 `---` frontmatter 开头".into()),
            Some(ys) if is_delim => return Ok((&text[ys..start], &text[offset..])),
            Some(_) => {}
        }
    }
    Err("frontmatter 没有闭合的 `---`".into())
}
```

`set` 现在总是返回 `Ok`，Task 3 会在这里加上 `type` 登记检查，所以先保留 `Result`。

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p towermd-core okf`
Expected: 7 passed。

- [ ] **Step 5: Commit**

```bash
git add crates/towermd-core/src/okf.rs crates/towermd-core/src/lib.rs
git commit -m "Core: OKF document parse with byte-identical round trip"
```

---

### Task 3: type 登记与层级（FR-01、FR-82）

**Files:**
- Create: `crates/towermd-core/src/types.rs`
- Modify: `crates/towermd-core/src/okf.rs`（`set` 加检查，新增 `Document::new`）
- Modify: `crates/towermd-core/src/lib.rs`

行为约定：
- `TYPES` 是 `type` 取值的唯一登记处，取值与规格 6.2 节的表一一对应。
- 每个 type 带一个默认层，只在对象缺 `layer` 字段或字段不合法时使用；对象自己的 `layer` 字段优先（FR-01）。
- `Decision` 可以落在 L1–L4，默认层取 L1。理由：原则冲突时慢层保护优先，缺字段的决策按慢层对待最安全。
- 写入路径（`Document::new`、`set("type", …)`）只接受已登记的 type；读取不检查。

- [ ] **Step 1: 写失败的测试**

`crates/towermd-core/src/types.rs`:

```rust
//! 对象 `type` 的唯一登记处（FR-82）与层级（FR-01）。

use crate::okf::Document;

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        Document::parse(text).unwrap()
    }

    #[test]
    fn registry_matches_spec_table() {
        let names: Vec<&str> = TYPES.iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "Intent", "Module", "Interface", "Acceptance", "Baseline", "Decision", "Proposal",
                "Task", "Run", "Evidence", "Hold", "Gate", "SkillPack", "Rule",
            ]
        );
    }

    #[test]
    fn lookup_known_and_unknown() {
        assert_eq!(lookup("Module").unwrap().default_layer, Layer::L1);
        assert_eq!(lookup("Acceptance").unwrap().default_layer, Layer::L2);
        assert!(lookup("module").is_none());
        assert!(lookup("FutureThing").is_none());
    }

    #[test]
    fn layer_field_wins_over_default() {
        let d = doc("---\ntype: Decision\nlayer: L4\n---\n");
        assert_eq!(layer_of(&d), Some(Layer::L4));
    }

    #[test]
    fn missing_or_invalid_layer_falls_back_to_default() {
        assert_eq!(layer_of(&doc("---\ntype: Decision\n---\n")), Some(Layer::L1));
        assert_eq!(layer_of(&doc("---\ntype: Task\nlayer: L9\n---\n")), Some(Layer::L4));
    }

    #[test]
    fn unknown_type_without_layer_has_no_layer() {
        assert_eq!(layer_of(&doc("---\ntype: FutureThing\n---\n")), None);
        assert_eq!(layer_of(&doc("---\ntype: FutureThing\nlayer: L2\n---\n")), Some(Layer::L2));
    }

    #[test]
    fn slow_layers_are_l0_to_l2() {
        assert!(Layer::L2.is_slow());
        assert!(!Layer::L3.is_slow());
    }

    #[test]
    fn writes_require_registered_type() {
        assert!(Document::new("Module", "正文\n").is_ok());
        assert!(Document::new("FutureThing", "").is_err());
        let mut d = doc("---\ntype: FutureThing\n---\n");
        assert!(d.set("title", "x".into()).is_ok(), "未知 type 的对象仍可改其他字段");
        assert!(d.set("type", "Bogus".into()).is_err());
        assert!(d.set("type", "Module".into()).is_ok());
    }

    #[test]
    fn new_document_renders_minimal_frontmatter() {
        let d = Document::new("Intent", "一句话。\n").unwrap();
        assert_eq!(d.to_text(), "---\ntype: Intent\n---\n一句话。\n");
    }
}
```

在 `crates/towermd-core/src/lib.rs` 末尾加：

```rust
pub mod types;
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p towermd-core types`
Expected: 编译失败，报 `cannot find value TYPES`、`Layer`、`lookup`、`layer_of`、`Document::new`。

- [ ] **Step 3: 实现 types.rs**

在 `types.rs` 的 `use` 之后、测试模块之前插入:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    L0,
    L1,
    L2,
    L3,
    L4,
    L5,
}

impl Layer {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "L0" => Self::L0,
            "L1" => Self::L1,
            "L2" => Self::L2,
            "L3" => Self::L3,
            "L4" => Self::L4,
            "L5" => Self::L5,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::L0 => "L0",
            Self::L1 => "L1",
            Self::L2 => "L2",
            Self::L3 => "L3",
            Self::L4 => "L4",
            Self::L5 => "L5",
        }
    }

    /// 慢层：只能经人批准修改（FR-02）。
    pub fn is_slow(self) -> bool {
        self <= Self::L2
    }
}

#[derive(Debug)]
pub struct TypeInfo {
    pub name: &'static str,
    /// 对象缺 `layer` 字段时使用的层；对象自己的 `layer` 优先（FR-01）。
    pub default_layer: Layer,
}

/// 规格 6.2 节对象表。新增 type 只改这里。
pub const TYPES: &[TypeInfo] = &[
    TypeInfo { name: "Intent", default_layer: Layer::L0 },
    TypeInfo { name: "Module", default_layer: Layer::L1 },
    TypeInfo { name: "Interface", default_layer: Layer::L1 },
    TypeInfo { name: "Acceptance", default_layer: Layer::L2 },
    TypeInfo { name: "Baseline", default_layer: Layer::L1 },
    // 决策可在 L1–L4；缺字段时按慢层对待，慢层保护优先。
    TypeInfo { name: "Decision", default_layer: Layer::L1 },
    TypeInfo { name: "Proposal", default_layer: Layer::L4 },
    TypeInfo { name: "Task", default_layer: Layer::L4 },
    TypeInfo { name: "Run", default_layer: Layer::L5 },
    TypeInfo { name: "Evidence", default_layer: Layer::L5 },
    TypeInfo { name: "Hold", default_layer: Layer::L5 },
    TypeInfo { name: "Gate", default_layer: Layer::L3 },
    TypeInfo { name: "SkillPack", default_layer: Layer::L3 },
    TypeInfo { name: "Rule", default_layer: Layer::L3 },
];

pub fn lookup(name: &str) -> Option<&'static TypeInfo> {
    TYPES.iter().find(|t| t.name == name)
}

/// 对象的有效层：`layer` 字段优先，其次登记的默认层；未知 type 又无 `layer` 时为 `None`。
pub fn layer_of(doc: &Document) -> Option<Layer> {
    doc.get_str("layer")
        .and_then(Layer::parse)
        .or_else(|| doc.doc_type().and_then(lookup).map(|t| t.default_layer))
}
```

- [ ] **Step 4: 在 okf.rs 接上写入检查**

在 `okf.rs` 顶部 `use` 之后加：

```rust
use crate::types;
```

把 `set` 替换为：

```rust
    /// 写入一个 frontmatter 字段；已有键原位替换，新键追加在末尾。
    /// 写 `type` 时只接受已登记的取值（FR-82）。
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        if key == "type" {
            let name = value.as_str().unwrap_or_default();
            if types::lookup(name).is_none() {
                return Err(format!("未登记的对象类型 `{name}`"));
            }
        }
        self.front.insert(Value::from(key), value);
        self.original = None;
        Ok(())
    }
```

在 `impl Document` 中 `parse` 之后加：

```rust
    /// 新建对象文档；`doc_type` 必须已登记（FR-82）。
    pub fn new(doc_type: &str, body: &str) -> Result<Self, String> {
        let mut doc = Self { front: Mapping::new(), body: body.to_string(), original: None };
        doc.set("type", Value::from(doc_type))?;
        Ok(doc)
    }
```

- [ ] **Step 5: 运行全部核心测试，确认通过**

Run: `cargo test -p towermd-core`
Expected: okf 7 个、types 8 个，全部 passed。

- [ ] **Step 6: Commit**

```bash
git add crates/towermd-core/src/types.rs crates/towermd-core/src/okf.rs crates/towermd-core/src/lib.rs
git commit -m "Core: single type registry with default layers; writes require registered type"
```

---

### Task 4: 配置 `.towermd/config.yaml`

**Files:**
- Create: `crates/towermd-core/src/config.rs`
- Modify: `crates/towermd-core/src/lib.rs`

行为约定：
- 必填 `project_id`、`docs_dir`，都是非空字符串。新建配置时写入 `hub: local`。
- 其他键（角色、并发、预算等，后续里程碑会加）原样保留，键序不变。
- `hub` 的读取放到 M0-2，本任务只负责写默认值。

- [ ] **Step 1: 写失败的测试**

`crates/towermd-core/src/config.rs`:

```rust
//! `.towermd/config.yaml`：项目 ID、用户文档目录、hub 地址（FR-97、FR-98）。不含任何密钥。

use std::fs;
use std::path::Path;

use serde_yaml::{Mapping, Value};

pub const CONFIG_PATH: &str = ".towermd/config.yaml";
pub const DEFAULT_DOCS_DIR: &str = "docs/tower";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_config_round_trips() {
        let cfg = Config::new("0192f0aa-0000-7000-8000-000000000001", "docs/tower");
        let text = cfg.to_yaml();
        assert_eq!(
            text,
            "project_id: 0192f0aa-0000-7000-8000-000000000001\ndocs_dir: docs/tower\nhub: local\n"
        );
        let back = Config::parse(&text).unwrap();
        assert_eq!(back.project_id(), "0192f0aa-0000-7000-8000-000000000001");
        assert_eq!(back.docs_dir(), "docs/tower");
    }

    #[test]
    fn unknown_keys_are_preserved_in_order() {
        let text = "budget: { usd: 5 }\nproject_id: p1\ndocs_dir: d\nhub: { url: wss://hub.example }\n";
        let cfg = Config::parse(text).unwrap();
        assert_eq!(cfg.to_yaml(), "budget:\n  usd: 5\nproject_id: p1\ndocs_dir: d\nhub:\n  url: wss://hub.example\n");
    }

    #[test]
    fn required_fields_are_checked() {
        assert!(Config::parse("docs_dir: d\n").is_err());
        assert!(Config::parse("project_id: p\n").is_err());
        assert!(Config::parse("project_id: ''\ndocs_dir: d\n").is_err());
        assert!(Config::parse("- a\n").is_err());
    }

    #[test]
    fn load_returns_none_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Config::load(dir.path()).unwrap().is_none());
        fs::create_dir_all(dir.path().join(".towermd")).unwrap();
        fs::write(dir.path().join(CONFIG_PATH), "project_id: p\ndocs_dir: d\n").unwrap();
        assert_eq!(Config::load(dir.path()).unwrap().unwrap().project_id(), "p");
    }
}
```

在 `lib.rs` 末尾加：

```rust
pub mod config;
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p towermd-core config`
Expected: 编译失败，报 `cannot find type Config`。

- [ ] **Step 3: 实现**

在 `config.rs` 的常量之后、测试模块之前插入:

```rust
#[derive(Debug, Clone)]
pub struct Config {
    map: Mapping,
}

impl Config {
    pub fn new(project_id: &str, docs_dir: &str) -> Self {
        let mut map = Mapping::new();
        map.insert("project_id".into(), project_id.into());
        map.insert("docs_dir".into(), docs_dir.into());
        map.insert("hub".into(), "local".into());
        Self { map }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let value: Value =
            serde_yaml::from_str(text).map_err(|e| format!("{CONFIG_PATH} 不是合法 YAML：{e}"))?;
        let Value::Mapping(map) = value else {
            return Err(format!("{CONFIG_PATH} 顶层必须是映射"));
        };
        let cfg = Self { map };
        for key in ["project_id", "docs_dir"] {
            if cfg.str_field(key).is_none() {
                return Err(format!("{CONFIG_PATH} 缺少非空的 `{key}`"));
            }
        }
        Ok(cfg)
    }

    /// 读取仓库根下的配置；文件不存在时返回 `None`。
    pub fn load(root: &Path) -> Result<Option<Self>, String> {
        let path = root.join(CONFIG_PATH);
        match fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("无法读取 {}：{e}", path.display())),
        }
    }

    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(&self.map).expect("YAML 映射总能序列化")
    }

    pub fn project_id(&self) -> &str {
        self.str_field("project_id").expect("parse 已检查")
    }

    pub fn docs_dir(&self) -> &str {
        self.str_field("docs_dir").expect("parse 已检查")
    }

    fn str_field(&self, key: &str) -> Option<&str> {
        self.map.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
    }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p towermd-core config`
Expected: 4 passed。如果 `unknown_keys_are_preserved_in_order` 只在缩进或流式写法上不符，以 serde_yaml 0.9 的实际输出为准修改期望字符串，但键序与键集合不得变。

- [ ] **Step 5: Commit**

```bash
git add crates/towermd-core/src/config.rs crates/towermd-core/src/lib.rs
git commit -m "Core: .towermd/config.yaml with project_id and docs_dir, preserving unknown keys"
```

---

### Task 5: `towermd init` 核心（FR-81、FR-98）

**Files:**
- Create: `crates/towermd-core/src/repo.rs`
- Create: `crates/towermd-core/src/init.rs`
- Create: `crates/towermd-core/templates/agents.md`
- Modify: `crates/towermd-core/src/lib.rs`

行为约定：
- `dir` 可以是仓库内任意路径，文件都写在它所在工作树的根目录（`git rev-parse --show-toplevel`）。不在 git 仓库中时报错。
- **首次执行**：用 `Uuid::now_v7()` 生成项目 ID；`docs_dir` 取参数，缺省为 `docs/tower`；写 `.towermd/config.yaml`。
- **再次执行**：配置文件一个字节都不改，项目 ID 和 docs_dir 沿用已有值。传入的 `docs_dir` 与已有值不同时报错，提示改动要走提案，因为配置属于 L3。
- `docs_dir` 必须是仓库内的相对目录，不能是 `.`、不能含 `..`，也不能落在 `.towermd/` 或 `.git/` 下。允许 `./` 前缀和结尾 `/`，写入时去掉。
- 首次执行时，如果所选目录下已有 `intents/`、`arch/`、`specs/`、`baselines/`、`state/` 或 `tower.canvas`，给出警告：这些会整体成为受保护路径（FR-17）。
- 文档骨架：`intents/`、`arch/`、`specs/`、`baselines/`、`decisions/` 中不存在的子目录，各建一个空的 `.gitkeep`；已有的目录不动。不建 `state/`，它只由快照写入者创建（FR-89、FR-94）。
- `.towermd/AGENTS.md`：不存在时按模板生成；已存在且内容与模板相同时不动；已存在但内容不同时不覆盖，只给警告。
- 仓库根目录的 `AGENTS.md` 只读不写：它没有引用 `.towermd/AGENTS.md` 时，给出提示，请人自己加一行。
- 不运行任何改变仓库状态的 git 命令：不 add，不 commit。

- [ ] **Step 1: 写 AGENTS.md 模板**

`crates/towermd-core/templates/agents.md`:

```markdown
# Tower.md 仓库规矩

本文件由 `towermd init` 生成，写给在本仓库里工作的任何 Agent。修改本文件属于 L3 流程变更，需要人登记。

## 慢层：只能经人在 Tower.md App 中批准修改

- `{docs_dir}/intents/`、`{docs_dir}/arch/`、`{docs_dir}/specs/`、`{docs_dir}/baselines/` 下的全部文件
- `{docs_dir}/tower.canvas`
- 其他位置 frontmatter 中 `layer` 为 `L0`–`L2` 的对象
- `{docs_dir}/state/`：快照，只由 Tower.md 守护进程写入
- `.towermd/` 下的配置

## 需要改慢层时

不要直接改。用 `towermd propose` 提出提案并附上 diff，经人批准后由 Tower.md 写进主干。合并时，凡是触及上述路径的 diff 都会被拒绝，并转成提案。

## 只能经 `towermd` 命令行读写的状态

任务、领取与租约、提问与回答、证据、提案都保存在 Tower.md hub 中，只能通过 `towermd` 命令行读写，例如 `towermd status`、`towermd task claim|show|release`、`towermd ask`、`towermd propose`、`towermd finish`。不要手写 `{docs_dir}/state/`，也不要伪造事件。

## 完成与合并

- 只改任务卡 `allowed_paths` 范围内的文件。
- 合并前必须运行 `towermd finish`，证据通过后才能合并。你自己报告"完成"不会改变任务状态。
```

- [ ] **Step 2: 写 repo.rs**

`crates/towermd-core/src/repo.rs`:

```rust
//! 直接调用 `git` 命令（不用 libgit2）。只读查询，不改仓库状态。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 在 `dir` 下运行 git，成功时返回去掉结尾换行的标准输出。
pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("无法运行 git：{e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} 失败：{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// `dir` 所在工作树的根目录。
pub fn toplevel(dir: &Path) -> Result<PathBuf, String> {
    git(dir, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .map_err(|_| format!("{} 不在 git 仓库中", dir.display()))
}
```

- [ ] **Step 3: 写 init.rs 的失败测试**

`crates/towermd-core/src/init.rs`:

```rust
//! `towermd init`（FR-98、FR-81）。hub 与守护进程尚不存在时运行，是唯一不经守护进程的写入：
//! 只在当前工作树生成文件，不提交，由人检查后自行提交。

use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::config::{Config, CONFIG_PATH, DEFAULT_DOCS_DIR};
use crate::repo;

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// 临时 git 仓库；不读用户的全局与系统 git 配置。
    fn git_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let ok = Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success();
        assert!(ok);
        dir
    }

    fn opts(docs_dir: Option<&str>) -> InitOptions {
        InitOptions { docs_dir: docs_dir.map(str::to_string) }
    }

    #[test]
    fn first_run_writes_config_agents_and_skeleton() {
        let repo = git_repo();
        let r = init(repo.path(), &opts(None)).unwrap();
        assert!(!r.reused_config);
        assert_eq!(r.docs_dir, "docs/tower");
        assert_eq!(Uuid::parse_str(&r.project_id).unwrap().get_version_num(), 7);

        let cfg = Config::load(repo.path()).unwrap().unwrap();
        assert_eq!(cfg.project_id(), r.project_id);
        let agents = fs::read_to_string(repo.path().join(AGENTS_PATH)).unwrap();
        assert!(agents.contains("`docs/tower/intents/`"));
        assert!(!agents.contains("{docs_dir}"));
        for sub in ["intents", "arch", "specs", "baselines", "decisions"] {
            assert!(repo.path().join(format!("docs/tower/{sub}/.gitkeep")).is_file(), "{sub}");
        }
        assert!(!repo.path().join("docs/tower/state").exists());
        assert!(r.created.contains(&CONFIG_PATH.to_string()));
        assert!(r.created.contains(&AGENTS_PATH.to_string()));
    }

    #[test]
    fn second_run_keeps_project_id_and_config_bytes() {
        let repo = git_repo();
        let first = init(repo.path(), &opts(Some("docs/plan"))).unwrap();
        let before = fs::read(repo.path().join(CONFIG_PATH)).unwrap();
        let second = init(repo.path(), &opts(None)).unwrap();
        assert!(second.reused_config);
        assert_eq!(second.project_id, first.project_id);
        assert_eq!(second.docs_dir, "docs/plan");
        assert!(second.created.is_empty(), "{:?}", second.created);
        assert_eq!(fs::read(repo.path().join(CONFIG_PATH)).unwrap(), before);
    }

    #[test]
    fn second_run_with_different_docs_dir_is_refused() {
        let repo = git_repo();
        init(repo.path(), &opts(Some("docs/a"))).unwrap();
        assert!(init(repo.path(), &opts(Some("docs/a/"))).is_ok());
        let err = init(repo.path(), &opts(Some("docs/b"))).unwrap_err();
        assert!(err.contains("docs/a"), "{err}");
    }

    #[test]
    fn runs_from_subdirectory_write_at_toplevel() {
        let repo = git_repo();
        let sub = repo.path().join("src/deep");
        fs::create_dir_all(&sub).unwrap();
        let r = init(&sub, &opts(None)).unwrap();
        assert_eq!(r.root.canonicalize().unwrap(), repo.path().canonicalize().unwrap());
        assert!(repo.path().join(CONFIG_PATH).is_file());
        assert!(!sub.join(".towermd").exists());
    }

    #[test]
    fn warns_when_docs_dir_has_protected_names() {
        let repo = git_repo();
        fs::create_dir_all(repo.path().join("docs/specs")).unwrap();
        fs::write(repo.path().join("docs/specs/old.md"), "旧规格\n").unwrap();
        let r = init(repo.path(), &opts(Some("docs"))).unwrap();
        assert!(r.warnings.iter().any(|w| w.contains("specs")), "{:?}", r.warnings);
        assert_eq!(fs::read_to_string(repo.path().join("docs/specs/old.md")).unwrap(), "旧规格\n");
        assert!(!repo.path().join("docs/specs/.gitkeep").exists(), "已有目录不动");
    }

    #[test]
    fn root_agents_md_is_never_modified() {
        let repo = git_repo();
        fs::write(repo.path().join("AGENTS.md"), "# 我的规矩\n").unwrap();
        let r = init(repo.path(), &opts(None)).unwrap();
        assert_eq!(fs::read_to_string(repo.path().join("AGENTS.md")).unwrap(), "# 我的规矩\n");
        assert!(r.hints.iter().any(|h| h.contains(AGENTS_PATH)), "{:?}", r.hints);

        fs::write(repo.path().join("AGENTS.md"), "见 .towermd/AGENTS.md\n").unwrap();
        let r = init(repo.path(), &opts(None)).unwrap();
        assert!(r.hints.is_empty(), "{:?}", r.hints);
    }

    #[test]
    fn edited_towermd_agents_md_is_not_overwritten() {
        let repo = git_repo();
        init(repo.path(), &opts(None)).unwrap();
        fs::write(repo.path().join(AGENTS_PATH), "人改过\n").unwrap();
        let r = init(repo.path(), &opts(None)).unwrap();
        assert_eq!(fs::read_to_string(repo.path().join(AGENTS_PATH)).unwrap(), "人改过\n");
        assert!(r.warnings.iter().any(|w| w.contains(AGENTS_PATH)), "{:?}", r.warnings);
    }

    #[test]
    fn does_not_commit() {
        let repo = git_repo();
        init(repo.path(), &opts(None)).unwrap();
        assert!(repo::git(repo.path(), &["rev-parse", "--verify", "HEAD"]).is_err());
    }

    #[test]
    fn outside_git_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = init(dir.path(), &opts(None)).unwrap_err();
        assert!(err.contains("不在 git 仓库中"), "{err}");
        assert!(!dir.path().join(".towermd").exists());
    }

    #[test]
    fn docs_dir_validation() {
        assert_eq!(normalize_docs_dir("./docs/tower/").unwrap(), "docs/tower");
        for bad in ["", ".", "/abs", "../up", "a/../b", ".towermd/x", ".git", "a//b", "a\\b"] {
            assert!(normalize_docs_dir(bad).is_err(), "{bad:?}");
        }
    }
}
```

在 `lib.rs` 末尾加：

```rust
pub mod init;
pub mod repo;
```

- [ ] **Step 4: 运行测试，确认失败**

Run: `cargo test -p towermd-core init`
Expected: 编译失败，报 `cannot find type InitOptions`、`init`、`AGENTS_PATH`、`normalize_docs_dir`。

- [ ] **Step 5: 实现**

在 `init.rs` 的 `use` 之后、测试模块之前插入:

```rust
pub const AGENTS_PATH: &str = ".towermd/AGENTS.md";
const AGENTS_TEMPLATE: &str = include_str!("../templates/agents.md");

/// 整体成为受保护路径的子目录（FR-17）。
const PROTECTED_SUBDIRS: &[&str] = &["intents", "arch", "specs", "baselines", "state"];
/// init 建立的文档骨架；不含 `state/`，它只由快照写入者创建（FR-89）。
const SKELETON: &[&str] = &["intents", "arch", "specs", "baselines", "decisions"];

#[derive(Debug, Default)]
pub struct InitOptions {
    pub docs_dir: Option<String>,
}

#[derive(Debug)]
pub struct InitReport {
    pub root: PathBuf,
    pub project_id: String,
    pub docs_dir: String,
    /// 已有配置被沿用（项目 ID 不变）。
    pub reused_config: bool,
    /// 本次新建的文件，相对仓库根。
    pub created: Vec<String>,
    pub warnings: Vec<String>,
    pub hints: Vec<String>,
}

pub fn render_agents_md(docs_dir: &str) -> String {
    AGENTS_TEMPLATE.replace("{docs_dir}", docs_dir)
}

/// 规范化用户文档目录：仓库内相对路径，不在 `.towermd/`、`.git/` 下。
pub fn normalize_docs_dir(raw: &str) -> Result<String, String> {
    let invalid = || format!("docs_dir `{raw}` 无效：须为仓库内的相对目录，不能是根目录，也不能在 .towermd/ 或 .git/ 下");
    let s = raw.trim();
    let s = s.strip_prefix("./").unwrap_or(s).trim_end_matches('/');
    if s.is_empty() || s.starts_with('/') || s.contains('\\') {
        return Err(invalid());
    }
    let parts: Vec<&str> = s.split('/').collect();
    if parts.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return Err(invalid());
    }
    if matches!(parts[0], ".towermd" | ".git") {
        return Err(invalid());
    }
    Ok(parts.join("/"))
}

pub fn init(dir: &Path, opts: &InitOptions) -> Result<InitReport, String> {
    let root = repo::toplevel(dir)?;
    let requested = opts.docs_dir.as_deref().map(normalize_docs_dir).transpose()?;
    let existing = Config::load(&root)?;
    let mut created = Vec::new();
    let mut warnings = Vec::new();
    let mut hints = Vec::new();

    let reused_config = existing.is_some();
    let config = match existing {
        Some(cfg) => {
            if let Some(d) = &requested {
                if d != cfg.docs_dir() {
                    return Err(format!(
                        "docs_dir 已设为 `{}`；init 不修改已有配置，改动请走提案",
                        cfg.docs_dir()
                    ));
                }
            }
            cfg
        }
        None => {
            let docs_dir = requested.unwrap_or_else(|| DEFAULT_DOCS_DIR.to_string());
            warnings.extend(protected_name_collisions(&root, &docs_dir));
            let cfg = Config::new(&Uuid::now_v7().to_string(), &docs_dir);
            write_new(&root, CONFIG_PATH, &cfg.to_yaml(), &mut created)?;
            cfg
        }
    };
    let docs_dir = config.docs_dir().to_string();

    for sub in SKELETON {
        let rel = format!("{docs_dir}/{sub}");
        if !root.join(&rel).exists() {
            write_new(&root, &format!("{rel}/.gitkeep"), "", &mut created)?;
        }
    }

    let agents = render_agents_md(&docs_dir);
    match fs::read_to_string(root.join(AGENTS_PATH)) {
        Ok(current) if current == agents => {}
        Ok(_) => warnings.push(format!("{AGENTS_PATH} 已存在且与生成内容不同，未覆盖")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            write_new(&root, AGENTS_PATH, &agents, &mut created)?
        }
        Err(e) => return Err(format!("无法读取 {AGENTS_PATH}：{e}")),
    }

    let root_agents = fs::read_to_string(root.join("AGENTS.md")).unwrap_or_default();
    if !root_agents.contains(AGENTS_PATH) {
        hints.push(format!(
            "请在仓库根目录的 AGENTS.md 中自行加一行：`Tower.md 的规矩见 {AGENTS_PATH}`（init 不修改该文件）"
        ));
    }

    Ok(InitReport {
        root,
        project_id: config.project_id().to_string(),
        docs_dir,
        reused_config,
        created,
        warnings,
        hints,
    })
}

fn protected_name_collisions(root: &Path, docs_dir: &str) -> Vec<String> {
    let base = root.join(docs_dir);
    let mut hits: Vec<&str> =
        PROTECTED_SUBDIRS.iter().copied().filter(|s| base.join(s).is_dir()).collect();
    if base.join("tower.canvas").exists() {
        hits.push("tower.canvas");
    }
    if hits.is_empty() {
        return Vec::new();
    }
    vec![format!(
        "`{docs_dir}` 下已有 {}，它们会整体成为受保护路径（只能经人批准修改）；建议另选一个专用目录",
        hits.join("、")
    )]
}

fn write_new(root: &Path, rel: &str, content: &str, created: &mut Vec<String>) -> Result<(), String> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建 {}：{e}", parent.display()))?;
    }
    fs::write(&path, content).map_err(|e| format!("无法写入 {}：{e}", path.display()))?;
    created.push(rel.to_string());
    Ok(())
}
```

- [ ] **Step 6: 运行全部核心测试，确认通过**

Run: `cargo test -p towermd-core`
Expected: okf 7、types 8、config 4、init 10，全部 passed。

- [ ] **Step 7: Commit**

```bash
git add crates/towermd-core/src/repo.rs crates/towermd-core/src/init.rs crates/towermd-core/templates/agents.md crates/towermd-core/src/lib.rs
git commit -m "Core: towermd init writes project id, docs_dir skeleton and .towermd/AGENTS.md without committing"
```

---

### Task 6: CLI `towermd init [path] [--docs-dir]`（16.1 节）

**Files:**
- Modify: `crates/towermd/src/main.rs`
- Create: `crates/towermd/tests/init_cli.rs`

行为约定：
- 没传 `--docs-dir`、仓库里也还没有配置、并且 stdin 是终端时，交互询问 docs_dir，直接回车取默认值（FR-98 "询问用户文档目录"）。stdin 不是终端时（Agent 或技能调用）不询问，直接用默认值；技能应显式传 `--docs-dir`。
- 输出项目 ID、docs_dir、新建文件、警告、提示，最后一行固定提醒：文件未提交，请检查后自行提交。
- 出错时向 stderr 打印 `towermd: <原因>`，退出码为 1。

- [ ] **Step 1: 写失败的集成测试**

`crates/towermd/tests/init_cli.rs`:

```rust
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let ok = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap()
        .success();
    assert!(ok);
    dir
}

fn towermd(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_towermd"))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn init_then_reinit_keeps_project_id() {
    let repo = git_repo();
    let first = towermd(repo.path(), &["init", "--docs-dir", "docs/plan"]);
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let out = stdout(&first);
    assert!(out.contains("新生成"), "{out}");
    assert!(out.contains("docs/plan"), "{out}");
    assert!(out.contains(".towermd/config.yaml"), "{out}");
    assert!(out.contains("未提交"), "{out}");

    let cfg = fs::read(repo.path().join(".towermd/config.yaml")).unwrap();
    let again = towermd(repo.path(), &["init"]);
    assert!(again.status.success());
    assert!(stdout(&again).contains("沿用"), "{}", stdout(&again));
    assert_eq!(fs::read(repo.path().join(".towermd/config.yaml")).unwrap(), cfg);
}

#[test]
fn path_argument_selects_the_repo() {
    let repo = git_repo();
    let elsewhere = tempfile::tempdir().unwrap();
    let o = towermd(elsewhere.path(), &["init", repo.path().to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(repo.path().join(".towermd/config.yaml").is_file());
    assert!(stdout(&o).contains("docs/tower"));
}

#[test]
fn non_git_directory_fails_with_reason() {
    let dir = tempfile::tempdir().unwrap();
    let o = towermd(dir.path(), &["init"]);
    assert_eq!(o.status.code(), Some(1));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.starts_with("towermd: "), "{err}");
    assert!(err.contains("不在 git 仓库中"), "{err}");
}

#[test]
fn changing_docs_dir_on_reinit_fails() {
    let repo = git_repo();
    assert!(towermd(repo.path(), &["init", "--docs-dir", "docs/a"]).status.success());
    let o = towermd(repo.path(), &["init", "--docs-dir", "docs/b"]);
    assert_eq!(o.status.code(), Some(1));
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p towermd --test init_cli`
Expected: 4 个全部 FAILED。现在的 `main` 是空的：不输出、不写文件，退出码总是 0。

- [ ] **Step 3: 实现 main.rs**

`crates/towermd/src/main.rs`:

```rust
//! `towermd` 命令行：只做参数解析与输出，业务逻辑在 towermd-core。

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use towermd_core::config::{Config, DEFAULT_DOCS_DIR};
use towermd_core::init::{init, InitOptions};
use towermd_core::repo;

#[derive(Parser)]
#[command(name = "towermd", version, about = "Tower.md：让 Agent 去做，只在关键处放行")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 初始化：生成项目 ID、.towermd/ 配置与 AGENTS.md、文档骨架（不提交）
    Init {
        /// 仓库内任意路径，默认当前目录
        path: Option<PathBuf>,
        /// 用户文档目录，相对仓库根（默认 docs/tower）
        #[arg(long)]
        docs_dir: Option<String>,
    },
}

fn main() -> ExitCode {
    let result = match Cli::parse().cmd {
        Cmd::Init { path, docs_dir } => run_init(path, docs_dir),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("towermd: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_init(path: Option<PathBuf>, docs_dir: Option<String>) -> Result<(), String> {
    let dir = match path {
        Some(p) => p,
        None => std::env::current_dir().map_err(|e| format!("无法读取当前目录：{e}"))?,
    };
    let docs_dir = match docs_dir {
        Some(d) => Some(d),
        None if io::stdin().is_terminal() && Config::load(&repo::toplevel(&dir)?)?.is_none() => {
            ask_docs_dir()?
        }
        None => None,
    };

    let r = init(&dir, &InitOptions { docs_dir })?;
    let origin = if r.reused_config { "沿用已有" } else { "新生成" };
    println!("项目 ID：{}（{origin}）", r.project_id);
    println!("用户文档目录：{}", r.docs_dir);
    if !r.created.is_empty() {
        println!("新建：");
        for f in &r.created {
            println!("  {f}");
        }
    }
    for w in &r.warnings {
        println!("警告：{w}");
    }
    for h in &r.hints {
        println!("提示：{h}");
    }
    println!("以上文件未提交，请检查后自行提交。");
    Ok(())
}

fn ask_docs_dir() -> Result<Option<String>, String> {
    print!("用户文档目录（相对仓库根）[{DEFAULT_DOCS_DIR}]: ");
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    let line = line.trim();
    Ok((!line.is_empty()).then(|| line.to_string()))
}
```

- [ ] **Step 4: 运行全部测试，确认通过**

Run: `cargo test --workspace`
Expected: 核心 29 个、CLI 4 个，全部 passed。

- [ ] **Step 5: 在真实终端手动验证交互询问**

在一个临时目录中运行：

```bash
tmp=$(mktemp -d) && git -C "$tmp" init -q && cargo run -q -p towermd -- init "$tmp"
```

Expected: 出现提示 `用户文档目录（相对仓库根）[docs/tower]: `。直接回车后，输出 `项目 ID：…（新生成）`；再运行一次，不再询问，输出 `（沿用已有）`。验证完删掉 `$tmp`。

- [ ] **Step 6: Commit**

```bash
git add crates/towermd/src/main.rs crates/towermd/tests/init_cli.rs
git commit -m "CLI: towermd init [path] [--docs-dir] with interactive prompt on TTY"
```

---

## 验收（对应规格 18 节 M0 中属于本计划的条目）

- [ ] `cargo test --workspace` 全部通过。
- [ ] `init` 生成 UUIDv7 项目 ID，再次执行时保持不变，配置文件逐字节不变（`second_run_keeps_project_id_and_config_bytes`、`init_then_reinit_keeps_project_id`）。
- [ ] 对象文件未改动时往返逐字节不变，未知键往返保留（`unmodified_round_trip_is_byte_identical`、`crlf_round_trip_is_byte_identical`、`edit_keeps_unknown_keys_order_and_body`）。
- [ ] type 只在 `types::TYPES` 一处登记，写入路径经过它（`writes_require_registered_type`）。
- [ ] init 不修改根目录 `AGENTS.md`，不提交（`root_agents_md_is_never_modified`、`does_not_commit`）。
- [ ] 把各次提交的 SHA 记进 `tasks/todo.md`。
