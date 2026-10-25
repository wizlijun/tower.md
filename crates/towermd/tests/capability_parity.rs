//! FR-190: every daemon route is reachable from the command line or the App,
//! canvas rendering and layout drags excepted. The capability-parity table in
//! `docs/implementation/capability-parity.md` lists each route and its
//! entries; this test keeps the table and the router in step.
use std::collections::BTreeSet;

const DAEMON: &str = include_str!("../src/daemon.rs");
const TABLE: &str = include_str!("../../../docs/implementation/capability-parity.md");
const SCHEMAS: &str = include_str!("../src/output/schema.rs");
const APP: &str = include_str!("../../../src-tauri/src/lib.rs");
const METHODS: &[&str] = &["get", "post", "put", "delete", "patch"];

/// (method, path) for every `.route(...)` in `fn router`, read from source so
/// the test needs no running daemon.
fn routes() -> BTreeSet<(String, String)> {
    let start = DAEMON.find("fn router(").expect("daemon.rs has fn router");
    let end = start + DAEMON[start..].find(".with_state(server)").expect("router ends with .with_state");
    let body = &DAEMON[start..end];
    let mut out = BTreeSet::new();
    for (at, _) in body.match_indices(".route(") {
        let rest = &body[at + ".route(".len()..];
        let open = rest.find('"').expect("route path literal");
        let close = open + 1 + rest[open + 1..].find('"').unwrap();
        let path = &rest[open + 1..close];
        // Walk the handler expression; a method is an identifier at depth 1
        // directly followed by `(`, e.g. `get(a).post(b)`.
        let (mut depth, mut word) = (1, String::new());
        for c in rest[close + 1..].chars() {
            match c {
                '(' => {
                    if depth == 1 && METHODS.contains(&word.as_str()) {
                        assert!(out.insert((word.to_uppercase(), path.to_owned())), "route registered twice: {word} {path}");
                    }
                    depth += 1;
                    word.clear();
                }
                ')' => {
                    depth -= 1;
                    word.clear();
                    if depth == 0 { break; }
                }
                c if c.is_ascii_alphanumeric() || c == '_' => word.push(c),
                _ => word.clear(),
            }
        }
    }
    out
}

struct Row { method: String, path: String, cells: [String; 4] }

fn rows() -> Vec<Row> {
    TABLE.lines().filter(|line| METHODS.iter().any(|m| line.starts_with(&format!("| {} |", m.to_uppercase())))).map(|line| {
        let cells: Vec<String> = line.trim().trim_start_matches('|').trim_end_matches('|').split('|').map(|cell| cell.trim().to_owned()).collect();
        assert_eq!(cells.len(), 7, "seven columns: {line}");
        let path = cells[1].trim_matches('`').to_owned();
        Row { method: cells[0].clone(), path, cells: [cells[2].clone(), cells[3].clone(), cells[4].clone(), cells[5].clone()] }
    }).collect()
}

fn names(cell: &str) -> Vec<&str> {
    cell.split('`').skip(1).step_by(2).collect()
}

fn app_commands() -> BTreeSet<&'static str> {
    let start = APP.find("generate_handler![").expect("lib.rs registers invokes") + "generate_handler![".len();
    let end = start + APP[start..].find(']').unwrap();
    APP[start..end].split(',').map(str::trim).filter(|name| !name.is_empty()).collect()
}

fn cli_commands() -> BTreeSet<&'static str> {
    SCHEMAS.lines().filter(|line| line.contains("include_str!(\"../../schema/")).filter_map(|line| line.split('"').nth(1)).collect()
}

#[test]
fn parser_reads_chained_and_multi_line_routes() {
    let routes = routes();
    for (method, path) in [("GET", "/projects/{handle}/tasks"), ("POST", "/projects/{handle}/tasks"), ("GET", "/health"), ("POST", "/projects/{handle}/baselines")] {
        assert!(routes.contains(&(method.to_owned(), path.to_owned())), "{method} {path}");
    }
}

#[test]
fn every_route_is_in_the_parity_table_and_nothing_else() {
    let routes = routes();
    let mut table = BTreeSet::new();
    for row in rows() {
        assert!(table.insert((row.method.clone(), row.path.clone())), "listed twice: {} {}", row.method, row.path);
    }
    let missing: Vec<_> = routes.difference(&table).collect();
    let stale: Vec<_> = table.difference(&routes).collect();
    assert!(missing.is_empty() && stale.is_empty(), "routes not in the table: {missing:?}; table rows with no route: {stale:?}");
}

#[test]
fn no_cell_is_empty_and_every_entry_exists() {
    let (app, cli) = (app_commands(), cli_commands());
    let gateway = [towermd_devkit::gateway::ASK, towermd_devkit::gateway::REVIEW, towermd_devkit::gateway::PERMISSION, towermd_devkit::gateway::NOTE];
    let reason = |cell: &str, lead: &str| cell.strip_prefix(lead).and_then(|rest| rest.strip_suffix('）')).is_some_and(|why| !why.trim().is_empty());
    for row in rows() {
        let at = format!("{} {}", row.method, row.path);
        for (column, cell) in row.cells.iter().enumerate() {
            let entries = names(cell);
            if cell.starts_with('≈') {
                assert!(!entries.is_empty() && cell.contains('（'), "{at}: an equivalent names its entry and says how: {cell}");
            } else if entries.is_empty() {
                let human = column == 1 && reason(cell, "仅 App（");
                assert!(human || reason(cell, "—（"), "{at}: an empty cell, or one without a reason: {cell:?}");
            }
            for entry in entries {
                match column {
                    0 => assert!(app.contains(entry), "{at}: no App invoke `{entry}`"),
                    1 => {
                        let name: Vec<_> = entry.split(' ').take_while(|word| !word.starts_with('-')).collect();
                        assert!(cli.contains(name.join(".").as_str()), "{at}: no command `{entry}`");
                    }
                    2 => assert!(gateway.contains(&entry), "{at}: no gateway tool `{entry}`"),
                    _ => assert!(serde_json::from_value::<towermd_core::tower_api::Verb>(serde_json::json!(entry)).is_ok(), "{at}: no Tower API verb `{entry}`"),
                }
            }
        }
        // A person's action is App-only by rule, so the App must reach it.
        if row.cells[1].starts_with("仅 App") {
            assert!(!names(&row.cells[0]).is_empty(), "{at}: App-only, yet the App has no entry");
        }
        assert!(row.cells.iter().any(|cell| !names(cell).is_empty()), "{at}: no entry reaches this route");
    }
}
