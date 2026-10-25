use super::*;
use crate::harness::{self, Permission};
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(root)
        .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" }).env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Pack Test", "-c", "user.email=pack@example.invalid", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null"])
        .args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn put(root: &Path, path: &str, text: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, text).unwrap();
}

fn commit(root: &Path) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "packs", "--allow-empty"]);
    git(root, &["rev-parse", "HEAD"])
}

const PACK: &str = ".towermd/skillpacks/flow@1.2.0";

/// The documented hash, computed independently of `digest`.
fn hash_of(dir: &Path) -> String {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() { walk(&path, base, out) } else {
                out.push((path.strip_prefix(base).unwrap().to_string_lossy().into_owned(), fs::read(&path).unwrap()));
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files);
    files.sort();
    let manifest: String = files.iter().map(|(path, bytes)| format!("{:x}  ./{path}\n", Sha256::digest(bytes))).collect();
    format!("sha256:{:x}", Sha256::digest(manifest.as_bytes()))
}

fn lock(hash: &str) -> String {
    format!("packs:\n  - type: SkillPack\n    name: flow\n    version: 1.2.0\n    origin: https://example.invalid/flow\n    hash: {hash}\n    enabled_for:\n      controller: [brainstorming]\n      executor: [writing-plans, tdd]\n    disabled: [worktrees]\n    interaction_points:\n      brainstorming: {{ maps_to: hold.question, scope: slow_layer }}\n      writing-plans: {{ maps_to: gate.G1 }}\n    note: kept\n")
}

/// A repo with one committed pack and its lock.
fn repo() -> (tempfile::TempDir, PathBuf, String) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    git(&root, &["init", "-qb", "main"]);
    for skill in ["brainstorming", "writing-plans", "tdd", "worktrees"] {
        put(&root, &format!("{PACK}/skills/{skill}/SKILL.md"), &format!("---\nname: {skill}\n---\n{skill}\n"));
    }
    put(&root, &format!("{PACK}/skills/tdd/run.sh"), "#!/bin/sh\necho tdd\n");
    put(&root, &format!("{PACK}/README.md"), "flow\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join(PACK).join("skills/tdd/run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let hash = hash_of(&root.join(PACK));
    put(&root, LOCK, &lock(&hash));
    let head = commit(&root);
    (temp, root, head)
}

fn config(dir: &Path) -> RunConfig {
    RunConfig { host: "h".into(), run: "R-1".into(), task: "T-1".into(), module: "A".into(), worktree: dir.join("wt"),
        run_dir: dir.join("runs/R-1"), task_card: "# T-1".into(), model: None, permission: Permission::WorkspaceWrite,
        budget_usd: None, timeout: Duration::from_secs(60), grace: Duration::from_secs(5), max_attempts: 1,
        mcp_config: None, permission_tool: None, skill_dirs: vec![], env: vec![], resume: None }
}

fn tree(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() { walk(&path, base, out) } else { out.push(path.strip_prefix(base).unwrap().to_string_lossy().into_owned()) }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn the_lock_reads_roles_disabled_skills_and_interaction_points() {
    let packs = parse(&lock(&format!("sha256:{}", "a".repeat(64)))).unwrap();
    assert_eq!(packs.len(), 1);
    let pack = &packs[0];
    assert_eq!((pack.name.as_str(), pack.version.as_str(), pack.path()), ("flow", "1.2.0", PACK.to_owned()));
    assert_eq!(pack.enabled_for[&Role::Executor], ["writing-plans", "tdd"]);
    assert!(pack.disabled.contains("worktrees"));
    assert_eq!(pack.interaction_points["brainstorming"], InteractionPoint { maps_to: "hold.question".into(), scope: Some("slow_layer".into()) });
    assert_eq!(parse("").unwrap(), Vec::new(), "no lock declares no packs");
    assert_eq!(parse("packs:\n").unwrap(), Vec::new());
}

#[test]
fn a_broken_lock_fails_closed() {
    let good = lock(&format!("sha256:{}", "a".repeat(64)));
    let broken = [
        ("- a\n", "顶层"),
        ("packs: {}\n", "列表"),
        (&good.replace("type: SkillPack", "type: Gate"), "type"),
        (&good.replace("    hash: sha256:", "    hash: md5:"), "hash"),
        (&good.replace("version: 1.2.0", "version: ../x"), "版本"),
        (&good.replace("name: flow", "name: a/b"), "技能包名"),
        (&good.replace("controller:", "planner:"), "角色"),
        (&good.replace("disabled: [worktrees]", "disabled: [tdd]"), "同时被启用和禁用"),
        (&good.replace("maps_to: gate.G1", "maps_to: auto.pass"), "交互点"),
        (&good.replace("[writing-plans, tdd]", "[../escape]"), "技能名"),
        (&format!("{good}{}", good.replace("packs:\n", "")), "两次"),
    ];
    for (text, why) in broken {
        let error = parse(text).unwrap_err();
        assert!(error.contains(why), "{why}: {error}");
    }
}

#[test]
fn the_documented_hash_matches_the_shell_recipe() {
    let (_temp, root, _) = repo();
    let Ok(output) = Command::new("sh").current_dir(root.join(PACK))
        .args(["-c", "find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256"]).output() else { return };
    if !output.status.success() { return }
    let printed = String::from_utf8(output.stdout).unwrap();
    assert_eq!(format!("sha256:{}", printed.split_whitespace().next().unwrap()), hash_of(&root.join(PACK)));
}

#[test]
fn a_role_gets_only_its_enabled_skills_from_the_committed_lock() {
    let (temp, root, head) = repo();
    let out = temp.path().join("out");
    let loaded = assemble(&root, &head, Role::Executor, &out).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!((loaded[0].name.as_str(), loaded[0].version.as_str(), loaded[0].skills.clone()), ("flow", "1.2.0", vec!["writing-plans".to_owned(), "tdd".into()]));
    assert_eq!(loaded[0].interaction_points.keys().collect::<Vec<_>>(), ["writing-plans"], "only the loaded skills' points");
    assert_eq!(tree(&out), ["flow/skills/tdd/SKILL.md", "flow/skills/tdd/run.sh", "flow/skills/writing-plans/SKILL.md"],
        "disabled and other roles' skills are left out");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(out.join("flow/skills/tdd/run.sh")).unwrap().permissions().mode() & 0o111, 0o111);
    }
    let data = serde_json::to_value(&loaded).unwrap();
    assert_eq!(data[0]["hash"], loaded[0].hash.as_str());
    assert!(data[0].get("dir").is_none(), "the event records no machine path");

    // The reviewer has nothing enabled; assembling again starts clean.
    assert!(assemble(&root, &head, Role::Reviewer, &out).unwrap().is_empty());
    assert!(!out.exists() || tree(&out).is_empty());

    // An uncommitted edit in the working tree changes nothing.
    put(&root, &format!("{PACK}/skills/tdd/SKILL.md"), "tampered\n");
    put(&root, LOCK, "packs: [oops\n");
    assert_eq!(assemble(&root, &head, Role::Executor, &out).unwrap(), loaded);
    assert_eq!(fs::read_to_string(out.join("flow/skills/tdd/SKILL.md")).unwrap(), "---\nname: tdd\n---\ntdd\n");
}

#[test]
fn an_altered_or_missing_copy_refuses_the_run() {
    let (temp, root, _) = repo();
    let out = temp.path().join("out");
    put(&root, &format!("{PACK}/skills/tdd/SKILL.md"), "changed without a new hash\n");
    let head = commit(&root);
    let error = assemble(&root, &head, Role::Executor, &out).unwrap_err();
    assert!(error.contains("哈希与锁定不符"), "{error}");

    let hash = hash_of(&root.join(PACK));
    put(&root, LOCK, &lock(&hash).replace("[writing-plans, tdd]", "[writing-plans, absent]"));
    let head = commit(&root);
    let error = assemble(&root, &head, Role::Executor, &out).unwrap_err();
    assert!(error.contains("没有技能 absent"), "{error}");

    git(&root, &["rm", "-rq", PACK]);
    put(&root, LOCK, &lock(&hash));
    let head = commit(&root);
    let error = assemble(&root, &head, Role::Executor, &out).unwrap_err();
    assert!(error.contains("未提交或为空"), "{error}");

    git(&root, &["rm", "-q", LOCK]);
    let head = commit(&root);
    assert!(assemble(&root, &head, Role::Executor, &out).unwrap().is_empty(), "no lock, no packs");
}

#[test]
fn wiring_passes_the_run_directory_copy_only_to_a_harness_with_a_skill_channel() {
    let (temp, root, head) = repo();
    for adapter in harness::adapters() {
        let mut run = config(&temp.path().join(adapter.id()));
        let loaded = wire(&mut run, adapter, &root, &head, Role::Executor).unwrap();
        let assembled = run.run_dir.join(ASSEMBLED).join("flow");
        let expected: Vec<PathBuf> = match adapter.capabilities().channels.skills {
            Skills::None => vec![],
            Skills::PluginDir => vec![assembled],
            Skills::SkillsDir => vec![assembled.join("skills")],
        };
        assert_eq!(run.skill_dirs, expected, "{}", adapter.id());
        assert_eq!(loaded.is_empty(), expected.is_empty(), "{}", adapter.id());
        for dir in &run.skill_dirs {
            assert!(dir.starts_with(&run.run_dir), "only the run directory is written");
        }
    }
    let mut run = config(temp.path());
    put(&root, LOCK, "packs: [oops\n");
    let head = commit(&root);
    for adapter in harness::adapters() {
        assert!(wire(&mut run, adapter, &root, &head, Role::Executor).is_err(), "a broken lock never runs, whatever the harness");
    }
}
