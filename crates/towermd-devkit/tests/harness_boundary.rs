//! Harness details stay in the adapters (AGENTS.md): no other production
//! source may name a harness's own flags or output format. The adapter
//! modules and their tests are the only exceptions.
use std::{fs, path::{Path, PathBuf}};

/// Flags and stream keys that belong to one harness or another.
const WORDS: [&str; 14] = [
    "stream-json", "total_cost_usd", "structured_output", "api_retry", "error_max_budget_usd",
    "--max-budget-usd", "--permission-prompt-tool", "--strict-mcp-config", "--plugin-dir",
    "thread.started", "turn.completed", "agent_message", "--output-schema", "--approval-mode",
];
const ADAPTERS: [&str; 5] = ["claude_code.rs", "codex.rs", "cursor.rs", "gemini.rs", "tests.rs"];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "node_modules" | "target" | "gen") {
                sources(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ["rs", "ts", "svelte"].contains(&ext.to_str().unwrap()))
            && !(path.parent().is_some_and(|parent| parent.ends_with("src/harness")) && ADAPTERS.contains(&name.as_str()))
        {
            out.push(path);
        }
    }
}

#[test]
fn only_adapters_name_harness_flags_and_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in ["crates/towermd-core/src", "crates/towermd-devkit/src", "crates/towermd/src", "src-tauri/src", "src"] {
        sources(&root.join(dir), &mut files);
    }
    files.sort();
    assert!(files.iter().any(|path| path.ends_with("src/runner.rs")), "扫描范围须包含运行器");
    let mut hits = Vec::new();
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        for (index, line) in text.lines().enumerate() {
            for word in WORDS.iter().filter(|word| line.contains(*word)) {
                hits.push(format!("{}:{}: {word}", path.strip_prefix(&root).unwrap_or(&path).display(), index + 1));
            }
        }
    }
    assert!(hits.is_empty(), "harness details outside the adapters:\n{}", hits.join("\n"));
}

#[test]
fn every_adapter_module_exists() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/harness");
    for name in ADAPTERS {
        assert!(dir.join(name).is_file(), "{name}");
    }
}
