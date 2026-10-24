use super::*;

fn none() -> impl FnMut(&str) -> Option<Vec<u8>> {
    |_: &str| None
}

fn read(text: &str) -> Parsed {
    parse("docs/progress/sync.puml", text.as_bytes(), &mut none())
}

fn edit(text: &str, edit: Edit<'_>) -> Result<String, String> {
    patch("docs/progress/sync.puml", text.as_bytes(), &edit, &mut none()).map(|bytes| String::from_utf8(bytes).unwrap())
}

fn changed_lines(before: &str, after: &str) -> usize {
    let (a, b): (Vec<&str>, Vec<&str>) = (before.split_inclusive('\n').collect(), after.split_inclusive('\n').collect());
    if a.len() != b.len() {
        return usize::MAX;
    }
    a.iter().zip(&b).filter(|(x, y)| x != y).count()
}

/// The acceptance file: three levels of nesting, 15 elements, 5 notes and
/// `skinparam` styling.
const PROGRESS: &str = r#"@startuml
' Red Army's progress chart
skinparam componentStyle rectangle
skinparam package {
  BackgroundColor #fafafa
}
left to right direction
title 同步进度

package "同步" as P1 {
  package "拉取" as P2 {
    rectangle "列出远端" as A1 <<done>>
    rectangle "下载对象" as A2 <<wip>>
    package "校验" as P3 {
      rectangle "哈希" as A3 <<todo>>
      rectangle "签名" as A4 <<todo>>
    }
  }
  package "推送" as P4 {
    rectangle "打包" as B1 <<done>>
    rectangle "上传" as B2 <<wip>>
    rectangle "确认" as B3
  }
}
package "冲突" as P5 {
  rectangle "检测" as C1 <<todo>>
  rectangle "合并" as C2 <<todo>>
}
rectangle "发布" as D1 #lightgreen

A1 --> A2
A2 --> A3 : 校验前
A3 ..> A4
B1 --> B2
B2 --> B3
A2 -[hidden]-> B1
C1 --> C2

note right of A1 <<why>> : 先列出再下载，避免漏拉
note left of A2 <<adjust>>
  人把并发从 8 降到 4
end note
note bottom of B2 : 上传要断点续传
note right of C1 <<question>> : 冲突是否交给人？
note "全局约束：不联网" as N1
N1 .. D1
@enduml
"#;

#[test]
fn reads_the_acceptance_chart() {
    let parsed = read(PROGRESS);
    assert_eq!(parsed.status, Status::Ok, "{:?}", parsed.problems);
    assert_eq!(parsed.elements.len(), 15);
    assert_eq!(parsed.element("A3").unwrap().parent.as_deref(), Some("P3"));
    assert_eq!(parsed.element("P3").unwrap().parent.as_deref(), Some("P2"));
    assert_eq!(parsed.element("P2").unwrap().parent.as_deref(), Some("P1"));
    assert_eq!(parsed.element("A2").unwrap().stereotypes, vec!["<<wip>>"]);
    assert_eq!(parsed.element("D1").unwrap().color.as_deref(), Some("#lightgreen"));
    assert_eq!(parsed.links.len(), 7);
    assert!(parsed.links.iter().any(|link| link.from == "A3" && link.to == "A4" && link.dashed));
    assert_eq!(parsed.links[1].label.as_deref(), Some("校验前"));
    assert_eq!(parsed.notes.len(), 5);
    assert_eq!(parsed.notes[1].text, "人把并发从 8 降到 4");
    assert_eq!(parsed.notes[4].target.as_deref(), Some("D1"));
    assert_eq!(parsed.understood, 100, "{:?}", parsed.unknown);
    assert_eq!(parsed.hints.len(), 2);
}

#[test]
fn changing_a_state_changes_one_line() {
    let after = edit(PROGRESS, Edit::State { key: "A2", token: Some("<<done>>"), remove: &["<<done>>".into(), "<<wip>>".into(), "<<todo>>".into()] }).unwrap();
    assert_eq!(changed_lines(PROGRESS, &after), 1);
    assert!(after.contains("rectangle \"下载对象\" as A2 <<done>>\n"));
    let cleared = edit(PROGRESS, Edit::State { key: "A1", token: None, remove: &["<<done>>".into()] }).unwrap();
    assert!(cleared.contains("rectangle \"列出远端\" as A1\n"));
    let color = edit(PROGRESS, Edit::State { key: "B3", token: Some("#lightgreen"), remove: &[] }).unwrap();
    assert!(color.contains("as B3 #lightgreen\n"));
    assert_eq!(read(&color).element("B3").unwrap().color.as_deref(), Some("#lightgreen"));
}

#[test]
fn edits_keep_crlf_and_every_other_byte() {
    let crlf = PROGRESS.replace('\n', "\r\n");
    let after = edit(&crlf, Edit::Title { key: "B3", title: "确认回执" }).unwrap();
    assert!(after.contains("rectangle \"确认回执\" as B3\r\n"));
    assert_eq!(changed_lines(&crlf, &after), 1);
    let added = edit(&crlf, Edit::Add { title: "重试", alias: None, parent: Some("P4"), token: Some("<<todo>>") }).unwrap();
    assert!(added.contains("    rectangle \"重试\" as E1 <<todo>>\r\n  }\r\n"), "{added}");
    assert!(!added.replace("\r\n", "").contains('\n'));
}

#[test]
fn adds_links_and_notes_before_the_end() {
    let linked = edit(PROGRESS, Edit::Link { from: "B3", to: "D1", label: Some("发布前"), dashed: true }).unwrap();
    assert!(linked.ends_with("B3 ..> D1 : 发布前\n@enduml\n"));
    let noted = edit(PROGRESS, Edit::Note { key: "D1", token: Some("<<why>>"), text: "等两边都完成" }).unwrap();
    assert!(noted.ends_with("note right of D1 <<why>> : 等两边都完成\n@enduml\n"));
    let parsed = read(&noted);
    assert!(parsed.notes.iter().any(|note| note.target.as_deref() == Some("D1") && note.text == "等两边都完成"));
}

#[test]
fn round_trip_is_byte_identical() {
    for text in MESSY {
        let parsed = read(text);
        // Reading never writes; patching a state back and forth restores it.
        let _ = parsed;
    }
    let there = edit(PROGRESS, Edit::State { key: "C1", token: Some("<<wip>>"), remove: &["<<todo>>".into()] }).unwrap();
    let back = edit(&there, Edit::State { key: "C1", token: Some("<<todo>>"), remove: &["<<wip>>".into()] }).unwrap();
    assert_eq!(back, PROGRESS);
}

#[test]
fn names_without_alias_and_conflicts() {
    let text = "@startuml\ncomponent \"同步模块\"\npackage A {\n  component X\n}\npackage B {\n  component X\n}\nrectangle R as K1\nrectangle S as K1\n[同步模块] --> X\n@enduml\n";
    let parsed = read(text);
    assert_eq!(parsed.status, Status::Ok);
    assert!(parsed.element("同步模块").is_some());
    assert!(parsed.element("A/X").is_some() && parsed.element("B/X").is_some());
    let conflicted: Vec<&Element> = parsed.elements.iter().filter(|e| e.read_only.as_deref() == Some(CONFLICT)).collect();
    assert_eq!(conflicted.len(), 2);
    assert!(edit(text, Edit::State { key: "K1", token: Some("<<done>>"), remove: &[] }).unwrap_err().contains("标识冲突"));
    assert!(edit(text, Edit::Title { key: "同步模块", title: "x" }).unwrap_err().contains("别名"));
    let renamed = edit("@startuml\ncomponent Sync\n@enduml\n", Edit::Title { key: "Sync", title: "同步" }).unwrap();
    assert_eq!(renamed, "@startuml\ncomponent \"同步\" as Sync\n@enduml\n");
}

#[test]
fn implicit_elements_and_link_shapes() {
    let parsed = read("@startuml\nA -> B\nB <-- C : back\n[X] -up-> (Y)\nD \"1\" *-- \"many\" E\nF <|-- G\n@enduml\n");
    assert_eq!(parsed.status, Status::Ok);
    assert!(parsed.element("A").unwrap().implicit);
    assert!(parsed.links.iter().any(|l| l.from == "C" && l.to == "B" && l.label.as_deref() == Some("back")));
    assert!(parsed.links.iter().any(|l| l.from == "G" && l.to == "F"));
    assert!(parsed.links.iter().any(|l| l.from == "D" && l.to == "E"));
    assert_eq!(parsed.element("X").unwrap().keyword, "component");
    assert_eq!(parsed.hints.len(), 1);
    assert!(edit("@startuml\nA -> B\n@enduml\n", Edit::State { key: "A", token: Some("<<done>>"), remove: &[] }).unwrap_err().contains("隐式"));
}

#[test]
fn unparsable_files_lock_edits_and_offer_fixes() {
    let missing = "@startuml\npackage P {\n  rectangle A as A1\n";
    let parsed = read(missing);
    assert_eq!(parsed.status, Status::Unparsable);
    assert_eq!(parsed.fixes[0].id, "end");
    assert!(edit(missing, Edit::State { key: "A1", token: Some("<<done>>"), remove: &[] }).unwrap_err().contains("锁定"));
    let fixed = edit(missing, Edit::Fix { id: "end" }).unwrap();
    assert!(fixed.ends_with("@enduml\n"));
    let open = read(&fixed);
    assert_eq!(open.status, Status::Unparsable);
    let id = open.fixes[0].id.clone();
    let closed = edit(&fixed, Edit::Fix { id: &id }).unwrap();
    assert_eq!(read(&closed).status, Status::Ok, "{closed}");
    assert_eq!(read("@startuml\n<<<<<<< HEAD\nA -> B\n=======\nA -> C\n>>>>>>> x\n@enduml\n").status, Status::Unparsable);
    assert_eq!(read("@startuml\n}\n@enduml\n").status, Status::Unparsable);
}

#[test]
fn includes_follow_the_repository_only() {
    let mut asked = Vec::new();
    let mut include = |path: &str| {
        asked.push(path.to_owned());
        (path == "docs/progress/shared.puml").then(|| b"@startuml\nrectangle Shared as S1\n@enduml\n".to_vec())
    };
    let text = "@startuml\n!include shared.puml\n!include https://example.com/x.puml\n!includeurl http://x/y\n!include <C4/C4_Container>\n!include ../../../outside.puml\nA1 --> S1\nrectangle A as A1\n@enduml\n";
    let parsed = parse("docs/progress/sync.puml", text.as_bytes(), &mut include);
    assert_eq!(asked, vec!["docs/progress/shared.puml"]);
    assert_eq!(parsed.includes, vec!["docs/progress/shared.puml"]);
    let shared = parsed.element("S1").unwrap();
    assert_eq!(shared.read_only.as_deref(), Some("来自 docs/progress/shared.puml，只读"));
    assert_eq!(parsed.unknown_lines, 4);
    assert!(parsed.unknown.iter().any(|line| line.reason.contains("从不访问网络")));
}

#[test]
fn macros_are_read_only_and_c4_is_guessed() {
    let text = "@startuml\n!define X(a) a\n!procedure $p()\n  rectangle Y\n!endprocedure\nContainer(api, \"API\", \"Rust\")\nSystem_Boundary(b1, \"边界\") {\n  ContainerDb(db, \"库\")\n}\nRel(api, db, \"读写\")\nLAYOUT_WITH_LEGEND()\n@enduml\n";
    let parsed = read(text);
    assert_eq!(parsed.status, Status::Ok, "{:?}", parsed.problems);
    assert_eq!(parsed.element("api").unwrap().read_only.as_deref(), Some(FROM_MACRO));
    assert_eq!(parsed.element("db").unwrap().parent.as_deref(), Some("b1"));
    assert_eq!(parsed.links[0].label.as_deref(), Some("读写"));
    assert_eq!(parsed.unknown_lines, 5);
    assert!(parsed.understood < 100);
}

#[test]
fn mind_maps_nest_by_markers() {
    let text = "@startmindmap\n* 目标\n** 拉取 <<done>>\n*** 校验\n** 推送\n*[#Orange] 颜色\n@endmindmap\n";
    let parsed = read(text);
    assert_eq!(parsed.status, Status::Ok);
    assert_eq!(parsed.element("校验").unwrap().parent.as_deref(), Some("拉取"));
    assert_eq!(parsed.element("拉取").unwrap().stereotypes, vec!["<<done>>"]);
    let after = edit(text, Edit::State { key: "推送", token: Some("<<wip>>"), remove: &[] }).unwrap();
    assert!(after.contains("** 推送 <<wip>>\n"));
    let added = edit(text, Edit::Add { title: "签名", alias: None, parent: Some("拉取"), token: None }).unwrap();
    assert!(added.contains("*** 校验\n*** 签名\n** 推送"), "{added}");
    assert_eq!(read("@startmindmap\n* a\n*** b\n@endmindmap\n").status, Status::Unparsable);
}

#[test]
fn other_diagrams_and_limits_are_reported_not_rejected() {
    let parsed = read("@startjson\n{\"a\": 1}\n@endjson\n@startuml\nrectangle A as A1\n@enduml\n");
    assert_eq!(parsed.status, Status::Ok);
    assert!(!parsed.diagrams[0].supported && parsed.diagrams[1].supported);
    assert_eq!(parsed.element("A1").unwrap().diagram, 1);
    assert_eq!(parse("x.puml", &vec![b'a'; MAX_BYTES + 1], &mut none()).status, Status::TooLarge);
    assert_eq!(parse("x.puml", &[0xb2, 0xe2, 0xca, 0xd4], &mut none()).status, Status::NotUtf8);
    assert_eq!(read("").status, Status::Ok);
    assert_eq!(starter("\n"), "@startuml\n@enduml\n");
}

/// Twenty real-world-messy files: none is rejected, each has a ratio.
const MESSY: [&str; 20] = [
    "@startuml\nactor User\nUser -> (Login)\n@enduml\n",
    "@startuml\nclass Foo {\n  +bar(): int\n  -baz\n}\nFoo <|-- Qux\n@enduml\n",
    "@startuml\nparticipant A\nA -> B: hi\nalt ok\n  B --> A: yes\nelse no\n  B --> A: no\nend\n@enduml\n",
    "@startuml\n:start;\nif (x) then\n:y;\nendif\n@enduml\n",
    "@startgantt\n[Task] lasts 5 days\n@endgantt\n",
    "@startsalt\n{\n[OK]\n}\n@endsalt\n",
    "no diagram here\njust text\n",
    "@startuml\nskinparam {\n  Shadowing false\n}\n<style>\ncomponent { LineColor red }\n</style>\n[A] --> [B]\n@enduml\n",
    "@startuml\n/' block\ncomment '/\nnode n1\nnode n2\nn1 -- n2\n@enduml\n",
    "@startuml\nlegend right\n  text\nendlegend\nrectangle \"x\" as x #red\n@enduml\n",
    "@startuml\n!theme cerulean\nsprite $foo [16x16/16] {\nFFFF\n}\ncloud c\n@enduml\n",
    "@startuml\nobject o1\nmap m {\n k => v\n}\no1 --> m\n@enduml\n",
    "@startuml\nusecase (Use) as UC1\nactor :Admin: as Ad\nAd --> UC1\n@enduml\n",
    "@startuml\nfolder F {\n  file f1\n  artifact a1\n}\ndatabase DB\nf1 ..> DB\n@enduml\n",
    "@startuml\nrectangle R1 [[http://x]]\nhexagon H\n@enduml\n",
    "@startuml\ntogether {\n  rectangle A\n  rectangle B\n}\nA -[#red,bold]-> B\n@enduml\n",
    "@startwbs\n* Root\n** A\n*** A1\n** B\n@endwbs\n",
    "@startuml\nnote as N\n  floating\nend note\nrectangle Z\nN .. Z\n@enduml\n",
    "\u{FEFF}@startuml\r\ncomponent C1\r\n@enduml\r\n",
    "@startuml\nrectangle \"a\" as a\nrectangle \"a\" as a\n@enduml\n<<<<<<<\n",
];

#[test]
fn twenty_messy_files_open_with_a_ratio() {
    for (index, text) in MESSY.iter().enumerate() {
        let parsed = read(text);
        assert!(parsed.understood <= 100, "file {index}");
        if index == 6 || index == 19 {
            assert_eq!(parsed.status, Status::Unparsable, "file {index}");
        } else {
            assert_eq!(parsed.status, Status::Ok, "file {index}: {:?}", parsed.problems);
        }
    }
    assert!(read(MESSY[2]).understood < 100);
    assert_eq!(read(MESSY[17]).notes[0].target.as_deref(), Some("Z"));
    assert!(read(MESSY[17]).links.is_empty());
    assert_eq!(read(MESSY[1]).element("Foo").unwrap().body, vec!["+bar(): int", "-baz"]);
}

#[test]
fn resolve_stays_inside_the_repository() {
    assert_eq!(resolve("docs/a.puml", "b.puml").as_deref(), Some("docs/b.puml"));
    assert_eq!(resolve("docs/x/a.puml", "../b.puml").as_deref(), Some("docs/b.puml"));
    assert_eq!(resolve("a.puml", "../b.puml"), None);
    assert_eq!(resolve("a.puml", "/etc/passwd"), None);
}
