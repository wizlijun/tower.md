//! The core must stay free of development-kit vocabulary, and must never
//! depend on the development kit crate.
use std::{fs, path::Path};

const WORDS: [&str; 6] = ["worktree", "module:", "G3", "G5", "task.merged", "harness"];
/// Literal git subcommand: the single writer must see every checkout of main.
const ALLOWED: [&str; 1] = [r#"&["worktree", "list", "--porcelain", "-z"],"#];

#[test]
fn core_sources_do_not_name_kit_concepts() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<_> = fs::read_dir(&src)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    files.sort();
    let mut hits = Vec::new();
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        for (index, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            if ALLOWED.contains(&line.trim()) {
                continue;
            }
            for word in WORDS {
                if line.contains(word) {
                    let name = path.file_name().unwrap().to_string_lossy();
                    hits.push(format!("{name}:{}: {word}", index + 1));
                }
            }
        }
    }
    assert!(hits.is_empty(), "core names kit concepts:\n{}", hits.join("\n"));
}

#[test]
fn core_manifest_does_not_depend_on_devkit() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    assert!(!manifest.contains("towermd-devkit"));
}
