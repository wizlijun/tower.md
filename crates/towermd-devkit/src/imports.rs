//! Line-oriented import extraction for the cold start S2 dependency graph.
//!
//! This is deliberately pattern matching, not parsing: each language has a
//! handful of statement shapes, and a specifier counts only when it resolves to
//! another tracked source file at the same fixed revision. External packages,
//! the standard library and anything unresolved are counted, never guessed.
//! The graph is derived data: the same files always yield the same edges.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Language {
    Rust,
    Script,
    Svelte,
    Python,
    Go,
}

impl Language {
    pub fn name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Script => "TypeScript/JavaScript",
            Self::Svelte => "Svelte",
            Self::Python => "Python",
            Self::Go => "Go",
        }
    }
}

pub fn language(path: &str) -> Option<Language> {
    match path.rsplit_once('.')?.1 {
        "rs" => Some(Language::Rust),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts" => Some(Language::Script),
        "svelte" => Some(Language::Svelte),
        "py" => Some(Language::Python),
        "go" => Some(Language::Go),
        _ => None,
    }
}

/// Counts that let the coverage recheck tell parsed evidence from fallback.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportSummary {
    /// Source files in a language this parser understands.
    pub supported_files: usize,
    /// Supported files with at least one resolved in- or outgoing edge.
    pub connected_files: usize,
    /// Import references that resolved to another tracked source file.
    pub resolved: usize,
    /// Import references that point outside the repository or did not resolve.
    pub external: usize,
    /// Supported files skipped because they were too large or not UTF-8.
    pub skipped: usize,
    pub languages: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default)]
pub struct ImportGraph {
    /// Directed file-to-file edges; the value is the number of statements.
    pub edges: BTreeMap<(String, String), usize>,
    pub summary: ImportSummary,
}

/// Build the graph from `files` (sorted source paths) and their text. `tracked`
/// is every tracked path, used to find `Cargo.toml` and `go.mod` roots.
pub fn graph(
    files: &BTreeSet<String>,
    tracked: &BTreeSet<String>,
    texts: &BTreeMap<String, Option<String>>,
) -> ImportGraph {
    let rust = RustCrates::new(tracked, texts);
    let go = go_modules(tracked, texts);
    let mut result = ImportGraph::default();
    let mut connected = BTreeSet::new();
    for file in files {
        let Some(language) = language(file) else {
            continue;
        };
        result.summary.supported_files += 1;
        *result
            .summary
            .languages
            .entry(language.name().into())
            .or_default() += 1;
        let Some(Some(text)) = texts.get(file) else {
            result.summary.skipped += 1;
            continue;
        };
        let targets = match language {
            Language::Rust => rust.imports(file, text, files),
            Language::Script | Language::Svelte => script_imports(file, text, files),
            Language::Python => python_imports(file, text, files),
            Language::Go => go_imports(file, text, files, &go),
        };
        for target in targets {
            match target {
                Some(target) if target != *file => {
                    result.summary.resolved += 1;
                    connected.insert(file.clone());
                    connected.insert(target.clone());
                    *result.edges.entry((file.clone(), target)).or_default() += 1;
                }
                Some(_) => {}
                None => result.summary.external += 1,
            }
        }
    }
    result.summary.connected_files = connected.len();
    result
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}
fn join(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.into()
    } else {
        format!("{directory}/{name}")
    }
}
/// POSIX-normalise `directory/relative`; `None` when it escapes the root.
fn normalize(directory: &str, relative: &str) -> Option<String> {
    let mut parts: Vec<&str> = directory.split('/').filter(|p| !p.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}
fn first(candidates: impl IntoIterator<Item = String>, files: &BTreeSet<String>) -> Option<String> {
    candidates.into_iter().find(|candidate| files.contains(candidate))
}
fn quoted(text: &str) -> Option<&str> {
    let quote = text.chars().next().filter(|c| matches!(c, '\'' | '"' | '`'))?;
    let rest = &text[1..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}
fn identifier(text: &str) -> &str {
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    &text[..end]
}

// --- TypeScript / JavaScript / Svelte ------------------------------------

const SCRIPT_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "svelte", "vue"];

/// `import … from 'x'`, `import 'x'`, `export … from 'x'`, `require('x')`,
/// `import('x')`. Only relative specifiers can name repository files.
fn script_imports(file: &str, text: &str, files: &BTreeSet<String>) -> Vec<Option<String>> {
    let mut specifiers = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if in_block {
            if trimmed.contains("*/") {
                in_block = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_block = true;
            continue;
        }
        if trimmed.starts_with("//") || trimmed.starts_with('*') {
            continue;
        }
        let statement = trimmed.starts_with("import ")
            || trimmed.starts_with("import{")
            || trimmed.starts_with("export ")
            || trimmed.starts_with('}')
            || trimmed.starts_with("from ");
        if statement {
            if let Some(index) = trimmed.rfind(" from ").map(|i| i + 6).or_else(|| {
                trimmed.strip_prefix("from ").map(|_| 5)
            }) {
                if let Some(spec) = quoted(trimmed[index..].trim_start()) {
                    specifiers.push(spec.to_owned());
                }
            } else if let Some(rest) = trimmed.strip_prefix("import ") {
                if let Some(spec) = quoted(rest.trim_start()) {
                    specifiers.push(spec.to_owned());
                }
            }
        }
        for call in ["require(", "import("] {
            let mut rest = trimmed;
            while let Some(index) = rest.find(call) {
                let before = rest[..index].chars().last();
                rest = &rest[index + call.len()..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                    continue;
                }
                if let Some(spec) = quoted(rest.trim_start()) {
                    specifiers.push(spec.to_owned());
                }
            }
        }
    }
    specifiers
        .into_iter()
        .map(|spec| resolve_script(file, &spec, files))
        .collect()
}

fn resolve_script(file: &str, spec: &str, files: &BTreeSet<String>) -> Option<String> {
    if !(spec.starts_with("./") || spec.starts_with("../")) {
        return None;
    }
    let spec = spec.split(['?', '#']).next().unwrap_or(spec);
    let base = normalize(parent(file), spec)?;
    let mut candidates = vec![base.clone()];
    // TypeScript ESM imports name the emitted `.js`; the source is `.ts`.
    for (emitted, sources) in [
        (".js", &["ts", "tsx"][..]),
        (".mjs", &["mts"][..]),
        (".cjs", &["cts"][..]),
        (".jsx", &["tsx"][..]),
    ] {
        if let Some(stem) = base.strip_suffix(emitted) {
            candidates.extend(sources.iter().map(|ext| format!("{stem}.{ext}")));
        }
    }
    candidates.extend(SCRIPT_EXTENSIONS.iter().map(|ext| format!("{base}.{ext}")));
    candidates.extend(SCRIPT_EXTENSIONS.iter().map(|ext| format!("{base}/index.{ext}")));
    first(candidates, files)
}

// --- Python ---------------------------------------------------------------

/// `import a.b[, c]` and `from [.]a.b import x[, y]`, absolute names resolved
/// against the importing file's ancestor directories (its likely source roots).
fn python_imports(file: &str, text: &str, files: &BTreeSet<String>) -> Vec<Option<String>> {
    let mut targets = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some(rest) = line.strip_prefix("import ") {
            for name in rest.split(',') {
                let name = name.split_whitespace().next().unwrap_or("");
                if !name.is_empty() {
                    targets.push(python_absolute(file, name, &[], files));
                }
            }
        } else if let Some(rest) = line.strip_prefix("from ") {
            let Some((module, names)) = rest.split_once(" import ") else {
                continue;
            };
            let module = module.trim();
            let names: Vec<&str> = names
                .trim()
                .trim_start_matches('(')
                .split(',')
                .filter_map(|name| name.split_whitespace().next())
                .map(|name| name.trim_end_matches(')'))
                .filter(|name| !name.is_empty() && *name != "*" && *name != "\\")
                .collect();
            if module.starts_with('.') {
                let dots = module.chars().take_while(|c| *c == '.').count();
                let mut directory = parent(file).to_owned();
                let mut escaped = false;
                for _ in 1..dots {
                    if directory.is_empty() {
                        escaped = true;
                        break;
                    }
                    directory = parent(&directory).to_owned();
                }
                if escaped {
                    targets.push(None);
                    continue;
                }
                let rest = &module[dots..];
                let package = if rest.is_empty() {
                    directory
                } else {
                    join(&directory, &rest.replace('.', "/"))
                };
                targets.push(python_package(&package, &names, files));
            } else {
                targets.push(python_absolute(file, module, &names, files));
            }
        }
    }
    targets
}

fn python_package(package: &str, names: &[&str], files: &BTreeSet<String>) -> Option<String> {
    // A submodule named in `from package import name` is more specific.
    names
        .iter()
        .find_map(|name| {
            first(
                [
                    join(package, &format!("{name}.py")),
                    join(package, &format!("{name}/__init__.py")),
                ],
                files,
            )
        })
        .or_else(|| {
            first(
                [format!("{package}.py"), join(package, "__init__.py")],
                files,
            )
        })
}

fn python_absolute(file: &str, module: &str, names: &[&str], files: &BTreeSet<String>) -> Option<String> {
    let relative = module.replace('.', "/");
    let mut directory = parent(file).to_owned();
    loop {
        if let Some(found) = python_package(&join(&directory, &relative), names, files) {
            return Some(found);
        }
        if directory.is_empty() {
            return None;
        }
        directory = parent(&directory).to_owned();
    }
}

// --- Go ---------------------------------------------------------------------

fn go_modules(tracked: &BTreeSet<String>, texts: &BTreeMap<String, Option<String>>) -> Vec<(String, String)> {
    let mut modules: Vec<(String, String)> = tracked
        .iter()
        .filter(|path| path.rsplit('/').next() == Some("go.mod"))
        .filter_map(|path| {
            let text = texts.get(path)?.as_ref()?;
            let module = text
                .lines()
                .find_map(|line| line.trim().strip_prefix("module "))?
                .trim()
                .trim_matches('"');
            Some((parent(path).to_owned(), module.to_owned()))
        })
        .collect();
    // The longest module path wins for nested modules.
    modules.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.cmp(b)));
    modules
}

/// `import "p"`, `import alias "p"` and `import ( … )` blocks. A package import
/// links the file to every Go file directly in the package directory.
fn go_imports(
    file: &str,
    text: &str,
    files: &BTreeSet<String>,
    modules: &[(String, String)],
) -> Vec<Option<String>> {
    let mut paths = Vec::new();
    let mut block = false;
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        if block {
            if line.starts_with(')') {
                block = false;
                continue;
            }
            if let Some(start) = line.find('"') {
                if let Some(path) = quoted(&line[start..]) {
                    paths.push(path.to_owned());
                }
            }
        } else if let Some(rest) = line
            .strip_prefix("import")
            .filter(|rest| rest.starts_with([' ', '\t', '(']))
        {
            let rest = rest.trim();
            if rest.starts_with('(') {
                block = !rest.contains(')');
                if let Some(start) = rest.find('"') {
                    if let Some(path) = quoted(&rest[start..]) {
                        paths.push(path.to_owned());
                    }
                }
            } else if let Some(start) = rest.find('"') {
                if let Some(path) = quoted(&rest[start..]) {
                    paths.push(path.to_owned());
                }
            }
        }
    }
    let mut targets = Vec::new();
    for path in paths {
        let directory = modules.iter().find_map(|(root, module)| {
            if path == *module {
                Some(root.clone())
            } else {
                path.strip_prefix(&format!("{module}/"))
                    .map(|rest| join(root, rest))
            }
        });
        let members: Vec<String> = directory
            .map(|directory| {
                files
                    .iter()
                    .filter(|candidate| {
                        parent(candidate) == directory && candidate.ends_with(".go") && candidate.as_str() != file
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if members.is_empty() {
            targets.push(None);
        } else {
            targets.extend(members.into_iter().map(Some));
        }
    }
    targets
}

// --- Rust -------------------------------------------------------------------

struct RustCrate {
    directory: String,
    name: String,
}

struct RustCrates {
    crates: Vec<RustCrate>,
}

impl RustCrates {
    fn new(tracked: &BTreeSet<String>, texts: &BTreeMap<String, Option<String>>) -> Self {
        let mut crates: Vec<RustCrate> = tracked
            .iter()
            .filter(|path| path.rsplit('/').next() == Some("Cargo.toml"))
            .filter_map(|path| {
                let text = texts.get(path)?.as_ref()?;
                let mut package = false;
                let mut name = None;
                for line in text.lines() {
                    let line = line.trim();
                    if line.starts_with('[') {
                        package = line == "[package]";
                    } else if package && name.is_none() {
                        if let Some(value) = line.strip_prefix("name") {
                            let value = value.trim_start();
                            if let Some(value) = value.strip_prefix('=') {
                                name = quoted(value.trim()).map(|n| n.replace('-', "_"));
                            }
                        }
                    }
                }
                Some(RustCrate { directory: parent(path).to_owned(), name: name? })
            })
            .collect();
        // Deepest crate first so nested crates own their files.
        crates.sort_by(|a, b| b.directory.len().cmp(&a.directory.len()).then_with(|| a.directory.cmp(&b.directory)));
        Self { crates }
    }

    fn owner(&self, file: &str) -> Option<&RustCrate> {
        self.crates.iter().find(|krate| {
            krate.directory.is_empty() || file.starts_with(&format!("{}/", krate.directory))
        })
    }

    fn source_root(krate: &RustCrate) -> String {
        join(&krate.directory, "src")
    }

    /// Module path of a file inside `src/`; `None` for crate roots outside it.
    fn module_path(krate: &RustCrate, file: &str) -> Option<Vec<String>> {
        let root = Self::source_root(krate);
        let relative = file.strip_prefix(&format!("{root}/"))?;
        if matches!(relative, "lib.rs" | "main.rs") {
            return Some(Vec::new());
        }
        let mut parts: Vec<String> = relative.split('/').map(str::to_owned).collect();
        let last = parts.pop()?;
        if parts.first().map(String::as_str) == Some("bin") {
            return None;
        }
        if last != "mod.rs" {
            parts.push(last.strip_suffix(".rs")?.to_owned());
        }
        Some(parts)
    }

    /// The file defining `segments` (longest prefix) under `root`.
    fn resolve(root: &str, segments: &[String], files: &BTreeSet<String>) -> Option<String> {
        for length in (0..=segments.len()).rev() {
            let prefix = &segments[..length];
            let candidates = if prefix.is_empty() {
                vec![join(root, "lib.rs"), join(root, "main.rs")]
            } else {
                let path = join(root, &prefix.join("/"));
                vec![format!("{path}.rs"), format!("{path}/mod.rs")]
            };
            if let Some(found) = first(candidates, files) {
                return Some(found);
            }
        }
        None
    }

    fn imports(&self, file: &str, text: &str, files: &BTreeSet<String>) -> Vec<Option<String>> {
        let krate = self.owner(file);
        let own = krate.and_then(|krate| Self::module_path(krate, file));
        let mut targets = Vec::new();
        let mut statement = String::new();
        let mut statement_nested = false;
        let mut in_block = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if in_block {
                if trimmed.contains("*/") {
                    in_block = false;
                }
                continue;
            }
            if trimmed.starts_with("/*") && !trimmed.contains("*/") {
                in_block = true;
                continue;
            }
            let code = trimmed.split("//").next().unwrap_or("").trim();
            if !statement.is_empty() {
                statement.push(' ');
                statement.push_str(code);
                if code.contains(';') {
                    targets.extend(self.use_targets(&statement, krate, own.as_deref(), statement_nested, files));
                    statement.clear();
                }
                continue;
            }
            let code = strip_visibility(code);
            // An indented `use super::` almost always sits in an inline
            // `mod tests`, whose parent is this file's own module.
            let nested = line.starts_with([' ', '\t']);
            if let Some(rest) = code.strip_prefix("mod ") {
                let name = identifier(rest.trim_start());
                if rest.trim_start()[name.len()..].trim_start().starts_with(';') && !name.is_empty() {
                    targets.push(child_module(file, name, files));
                }
            } else if let Some(rest) = code.strip_prefix("use ") {
                if rest.contains(';') {
                    targets.extend(self.use_targets(rest, krate, own.as_deref(), nested, files));
                } else {
                    statement = rest.to_owned();
                    statement_nested = nested;
                }
            }
        }
        targets
    }

    fn use_targets(
        &self,
        statement: &str,
        krate: Option<&RustCrate>,
        own: Option<&[String]>,
        nested: bool,
        files: &BTreeSet<String>,
    ) -> Vec<Option<String>> {
        let statement = statement.split(';').next().unwrap_or("").trim();
        expand_use(statement)
            .into_iter()
            .map(|path| {
                let segments: Vec<String> = path
                    .split("::")
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                let head = segments.first()?.as_str();
                let rest = &segments[1..];
                match head {
                    "crate" => {
                        let krate = krate?;
                        Self::resolve(&Self::source_root(krate), rest, files)
                    }
                    "self" | "super" => {
                        let krate = krate?;
                        let mut base = own?.to_vec();
                        let mut rest = rest;
                        if head == "super" && !nested {
                            base.pop()?;
                        }
                        while rest.first().map(String::as_str) == Some("super") {
                            base.pop()?;
                            rest = &rest[1..];
                        }
                        base.extend(rest.iter().cloned());
                        Self::resolve(&Self::source_root(krate), &base, files)
                    }
                    name => {
                        let target = self.crates.iter().find(|krate| krate.name == name)?;
                        Self::resolve(&Self::source_root(target), rest, files)
                    }
                }
            })
            .collect()
    }
}

fn strip_visibility(code: &str) -> &str {
    let Some(rest) = code.strip_prefix("pub") else {
        return code;
    };
    let rest = rest.trim_start();
    if let Some(inner) = rest.strip_prefix('(') {
        return inner.split_once(')').map(|(_, after)| after.trim_start()).unwrap_or(code);
    }
    rest
}

/// `mod name;` in `a/b.rs` lives at `a/b/name.rs`; in a crate or `mod.rs`
/// root it lives next to the declaring file.
fn child_module(file: &str, name: &str, files: &BTreeSet<String>) -> Option<String> {
    let directory = parent(file);
    let stem = file.rsplit('/').next()?.strip_suffix(".rs")?;
    let target_root = matches!(directory.rsplit('/').next(), Some("tests" | "examples" | "benches" | "bin"));
    let base = if target_root || matches!(stem, "mod" | "lib" | "main" | "build") {
        directory.to_owned()
    } else {
        join(directory, stem)
    };
    first(
        [join(&base, &format!("{name}.rs")), join(&base, &format!("{name}/mod.rs"))],
        files,
    )
}

/// Expand one level of `a::{b, c::d}` groups, which covers nearly every real
/// statement; deeper nesting keeps its outer prefix and still resolves.
fn expand_use(statement: &str) -> Vec<String> {
    let statement = statement.trim();
    let Some(open) = statement.find('{') else {
        return vec![statement.split(" as ").next().unwrap_or(statement).to_owned()];
    };
    let prefix = statement[..open].trim().trim_end_matches("::");
    let inner = &statement[open + 1..statement.rfind('}').unwrap_or(statement.len())];
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in inner.chars() {
        match ch {
            '{' => {
                depth += 1;
                current.push(ch);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => {
                items.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    items.push(current);
    items
        .into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .map(|item| {
            let item = item.split('{').next().unwrap_or(&item).trim().trim_end_matches("::").to_owned();
            let item = item.split(" as ").next().unwrap_or(&item).trim().to_owned();
            if item == "self" {
                prefix.to_owned()
            } else {
                format!("{prefix}::{item}")
            }
        })
        .collect()
}
