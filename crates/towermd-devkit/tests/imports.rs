use std::collections::{BTreeMap, BTreeSet};

use towermd_devkit::imports::{graph, ImportGraph};

fn build(files: &[(&str, &str)]) -> ImportGraph {
    let tracked: BTreeSet<String> = files.iter().map(|(path, _)| path.to_string()).collect();
    let sources: BTreeSet<String> = tracked
        .iter()
        .filter(|path| !path.ends_with("Cargo.toml") && !path.ends_with("go.mod"))
        .cloned()
        .collect();
    let texts: BTreeMap<String, Option<String>> = files
        .iter()
        .map(|(path, text)| (path.to_string(), Some(text.to_string())))
        .collect();
    graph(&sources, &tracked, &texts)
}

fn edges(graph: &ImportGraph) -> Vec<(&str, &str)> {
    graph.edges.keys().map(|(a, b)| (a.as_str(), b.as_str())).collect()
}

#[test]
fn rust_mod_use_crate_super_and_workspace_crates_resolve() {
    let graph = build(&[
        ("core/Cargo.toml", "[package]\nname = \"my-core\"\nversion = \"0.1.0\"\n"),
        ("core/src/lib.rs", "pub mod store;\nmod net;\n// mod commented;\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\n"),
        ("core/src/store.rs", "use crate::net::Client;\nuse std::fs;\n"),
        ("core/src/net/mod.rs", "pub(crate) mod wire;\n"),
        ("core/src/net/wire.rs", "use super::super::store;\nuse crate::{\n    store::Thing,\n    net::wire as w,\n};\n"),
        ("app/Cargo.toml", "[package]\nname = \"app\"\n[dependencies]\nname = \"ignored\"\n"),
        ("app/src/main.rs", "use my_core::store::Store;\nuse serde::Serialize;\n"),
        ("app/tests/flow.rs", "mod support;\nuse app_missing::x;\n"),
        ("app/tests/support/mod.rs", "pub fn help() {}\n"),
    ]);
    let found = edges(&graph);
    for expected in [
        ("core/src/lib.rs", "core/src/store.rs"),
        ("core/src/lib.rs", "core/src/net/mod.rs"),
        ("core/src/store.rs", "core/src/net/mod.rs"),
        ("core/src/net/mod.rs", "core/src/net/wire.rs"),
        ("core/src/net/wire.rs", "core/src/store.rs"),
        ("app/src/main.rs", "core/src/store.rs"),
        ("app/tests/flow.rs", "app/tests/support/mod.rs"),
    ] {
        assert!(found.contains(&expected), "missing {expected:?} in {found:?}");
    }
    assert!(!found.iter().any(|(_, to)| to.contains("commented")));
    // Indented `use super::*` in an inline test module points back at its own file.
    assert!(!found.contains(&("core/src/lib.rs", "core/src/lib.rs")));
    assert!(graph.summary.external >= 3, "{:?}", graph.summary);
    assert_eq!(graph.summary.languages["Rust"], 7);
}

#[test]
fn typescript_javascript_and_svelte_relative_specifiers_resolve() {
    let graph = build(&[
        ("src/main.ts", "import App from './App.svelte'\nimport { api } from './lib/api.js'\nimport './styles/app.css'\nimport x from 'svelte'\n"),
        ("src/App.svelte", "<script lang=\"ts\">\n  import {\n    a,\n    b,\n  } from './lib/util'\n  const C = await import('./lazy')\n</script>\n<p>import from './not-code'</p>\n"),
        ("src/lib/api.ts", "export * from './types'\nconst fs = require('fs')\nconst local = require(\"../legacy/index.cjs\")\n"),
        ("src/lib/types.ts", "export type T = 1\n"),
        ("src/lib/util/index.ts", "// import nothing from './gone'\nexport const a = 1\n"),
        ("src/lazy.tsx", "export default 1\n"),
        ("src/legacy/index.cjs", "module.exports = {}\n"),
        ("src/styles/app.css", "body {}\n"),
    ]);
    let found = edges(&graph);
    for expected in [
        ("src/main.ts", "src/App.svelte"),
        ("src/main.ts", "src/lib/api.ts"),
        ("src/main.ts", "src/styles/app.css"),
        ("src/App.svelte", "src/lib/util/index.ts"),
        ("src/App.svelte", "src/lazy.tsx"),
        ("src/lib/api.ts", "src/lib/types.ts"),
        ("src/lib/api.ts", "src/legacy/index.cjs"),
    ] {
        assert!(found.contains(&expected), "missing {expected:?} in {found:?}");
    }
    assert!(!found.iter().any(|(_, to)| to.contains("gone") || to.contains("not-code")));
    assert_eq!(graph.summary.external, 2, "svelte and fs are external");
}

#[test]
fn python_absolute_relative_and_package_imports_resolve() {
    let graph = build(&[
        ("pkg/__init__.py", ""),
        ("pkg/app.py", "import os\nimport pkg.models as m, json\nfrom .services import billing\nfrom . import utils  # helpers\n"),
        ("pkg/models.py", "from pkg.services.billing import charge\n"),
        ("pkg/services/__init__.py", ""),
        ("pkg/services/billing.py", "from ..utils import (\n    fmt,\n)\nfrom ... import beyond\n"),
        ("pkg/utils.py", "# import pkg.app\n"),
    ]);
    let found = edges(&graph);
    for expected in [
        ("pkg/app.py", "pkg/models.py"),
        ("pkg/app.py", "pkg/services/billing.py"),
        ("pkg/app.py", "pkg/utils.py"),
        ("pkg/models.py", "pkg/services/billing.py"),
        ("pkg/services/billing.py", "pkg/utils.py"),
    ] {
        assert!(found.contains(&expected), "missing {expected:?} in {found:?}");
    }
    assert!(!found.iter().any(|(from, _)| *from == "pkg/utils.py"));
}

#[test]
fn go_module_packages_resolve_through_go_mod() {
    let graph = build(&[
        ("go.mod", "module example.com/shop\n\ngo 1.22\n"),
        ("cmd/server/main.go", "package main\n\nimport (\n\t\"fmt\"\n\tstore \"example.com/shop/internal/store\"\n\t\"example.com/shop/internal/api\" // handlers\n)\n"),
        ("internal/store/db.go", "package store\nimport \"database/sql\"\n"),
        ("internal/store/cache.go", "package store\n"),
        ("internal/api/http.go", "package api\nimport \"example.com/shop/internal/store\"\nvar importer = 1\n"),
    ]);
    let found = edges(&graph);
    for expected in [
        ("cmd/server/main.go", "internal/store/db.go"),
        ("cmd/server/main.go", "internal/store/cache.go"),
        ("cmd/server/main.go", "internal/api/http.go"),
        ("internal/api/http.go", "internal/store/db.go"),
    ] {
        assert!(found.contains(&expected), "missing {expected:?} in {found:?}");
    }
    assert_eq!(graph.summary.external, 2, "fmt and database/sql");
}

#[test]
fn the_same_input_always_yields_the_same_graph() {
    let files = [
        ("src/a.ts", "import './b'\n"),
        ("src/b.ts", "import './a'\n"),
        ("lib/x.py", "from . import y\n"),
        ("lib/y.py", ""),
    ];
    let first = build(&files);
    for _ in 0..5 {
        let again = build(&files);
        assert_eq!(again.edges, first.edges);
        assert_eq!(again.summary, first.summary);
    }
    assert_eq!(first.summary.supported_files, 4);
    assert_eq!(first.summary.connected_files, 4);
}
