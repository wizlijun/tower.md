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
    assert_eq!(read(MESSY[2]).understood, 100);
    assert!(read(MESSY[2]).diagrams[0].sequence);
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

/// FR-172 phase 2: participants are elements and messages are links in file
/// order; groups, activations, separators and delays are kept, not drawn.
const SEQUENCE_FILE: &str = r#"@startuml
autonumber
actor 用户 as U
box "后端" #eeeeee
  participant "网关" as G <<done>>
  database "库" as D
end box
U -> G : 登录
activate G
alt 成功
  G -> D : 查询
  D --> G : 行
else 失败
  G ->x D
end
== 第二阶段 ==
...
||45||
ref over G, D
  共享流程
end ref
create participant "审计" as A
G -> A : 记一笔
return 完成
deactivate G
note over U, G : 读者看得到
[-> U : 外部
@enduml
"#;

#[test]
fn a_sequence_diagram_reads_participants_and_messages_in_order() {
    let parsed = read(SEQUENCE_FILE);
    assert_eq!(parsed.status, Status::Ok, "{:?}", parsed.problems);
    assert!(parsed.diagrams[0].sequence);
    let keys: Vec<(&str, &str, &str)> = parsed.elements.iter().map(|e| (e.key.as_str(), e.name.as_str(), e.keyword.as_str())).collect();
    assert_eq!(keys, [("U", "用户", "actor"), ("G", "网关", "participant"), ("D", "库", "database"), ("A", "审计", "participant")]);
    assert_eq!(parsed.element("G").unwrap().stereotypes, ["<<done>>"]);
    assert!(parsed.elements.iter().all(|e| e.parent.is_none()), "a box groups nothing on the canvas");
    let links: Vec<(&str, &str, Option<&str>, bool, usize)> = parsed.links.iter()
        .map(|l| (l.from.as_str(), l.to.as_str(), l.label.as_deref(), l.dashed, l.line)).collect();
    assert_eq!(links, [
        ("U", "G", Some("登录"), false, 8),
        ("G", "D", Some("查询"), false, 11),
        ("D", "G", Some("行"), true, 12),
        ("G", "D", None, false, 14),
        ("G", "A", Some("记一笔"), false, 23),
        ("[", "U", Some("外部"), false, 27),
    ]);
    assert_eq!(parsed.links[3].head, Some("lost"));
    assert!(parsed.links[5].boundary && !parsed.links[0].boundary);
    assert_eq!(parsed.notes[0].target.as_deref(), Some("U"));
    assert!(parsed.unknown.is_empty(), "{:?}", parsed.unknown);
    assert_eq!(parsed.understood, 100);
    // A component diagram's `-->` stays solid.
    let component = read("@startuml\n[A] --> [B]\n@enduml\n");
    assert!(!component.diagrams[0].sequence && !component.links[0].dashed);
}

#[test]
fn an_unbalanced_sequence_group_locks_the_file() {
    for (text, reason) in [
        ("@startuml\nparticipant A\nalt x\nA -> A\n@enduml\n", "时序图有 1 个块没有 end（最外层是第 3 行的 alt）"),
        ("@startuml\nparticipant A\nend\n@enduml\n", "end 前没有对应的块"),
        ("@startuml\nparticipant A\nelse\n@enduml\n", "else 前没有对应的 alt 块"),
        ("@startuml\nbox\nparticipant A\nend\n@enduml\n", "end 与第 2 行的 box 不配对"),
    ] {
        let parsed = read(text);
        assert_eq!(parsed.status, Status::Unparsable, "{text}");
        assert!(parsed.problems.iter().any(|problem| problem.reason == reason), "{text}: {:?}", parsed.problems);
    }
}

#[test]
fn the_canvas_writes_participants_and_messages_into_a_sequence_diagram() {
    let text = "@startuml\nparticipant \"网关\" as G\nG -> G : 自检\n@enduml\n";
    let added = edit(text, Edit::Add { title: "库", alias: None, parent: None, token: Some("<<todo>>") }).unwrap();
    assert_eq!(added, "@startuml\nparticipant \"网关\" as G\nG -> G : 自检\nparticipant \"库\" as E1 <<todo>>\n@enduml\n");
    let linked = edit(&added, Edit::Link { from: "G", to: "E1", label: Some("查询"), dashed: false }).unwrap();
    let replied = edit(&linked, Edit::Link { from: "E1", to: "G", label: None, dashed: true }).unwrap();
    assert!(replied.ends_with("G -> E1 : 查询\nE1 --> G\n@enduml\n"), "{replied}");
    let parsed = read(&replied);
    let links: Vec<(&str, &str, bool)> = parsed.links.iter().map(|l| (l.from.as_str(), l.to.as_str(), l.dashed)).collect();
    assert_eq!(links, [("G", "G", false), ("G", "E1", false), ("E1", "G", true)]);
    let state = edit(&replied, Edit::State { key: "E1", token: Some("<<done>>"), remove: &["<<todo>>".to_owned()] }).unwrap();
    assert_eq!(changed_lines(&replied, &state), 1);
    assert_eq!(read(&state).element("E1").unwrap().stereotypes, ["<<done>>"]);
    assert_eq!(edit(text, Edit::Add { title: "子", alias: None, parent: Some("G"), token: None }).unwrap_err(), "时序图的参与方没有上级");
}

#[test]
fn sequence_heads_boundaries_and_activations_are_read() {
    let text = "@startuml\nparticipant A\nparticipant B\nA ->> B : 异步\nA -\\ B\nB //-- A : 回\nA -/ B\nA ->o B\nA -> B ++ : 激活\nB --> A -- : 完\n[-> A : 进\n?-> B\nA ->] : 出\nB -->x] : 丢\nA <-] : 回来\n[<- B\nA -> A : 自己\n@enduml\n";
    let parsed = read(text);
    assert_eq!(parsed.status, Status::Ok, "{:?}", parsed.problems);
    assert!(parsed.unknown.is_empty(), "{:?}", parsed.unknown);
    let links: Vec<(&str, &str, Option<&str>, bool, Option<&str>)> = parsed.links.iter()
        .map(|l| (l.from.as_str(), l.to.as_str(), l.head, l.dashed, l.label.as_deref())).collect();
    assert_eq!(links, [
        ("A", "B", Some("async"), false, Some("异步")),
        ("A", "B", Some("half"), false, None),
        ("A", "B", Some("half"), true, Some("回")),
        ("A", "B", Some("half"), false, None),
        ("A", "B", Some("circle"), false, None),
        ("A", "B", None, false, Some("激活")),
        ("B", "A", None, true, Some("完")),
        ("[", "A", None, false, Some("进")),
        ("[", "B", None, false, None),
        ("A", "]", None, false, Some("出")),
        ("B", "]", Some("lost"), true, Some("丢")),
        ("]", "A", None, false, Some("回来")),
        ("B", "[", None, false, None),
        ("A", "A", None, false, Some("自己")),
    ]);
    assert_eq!(parsed.links.iter().filter(|l| l.boundary).count(), 6);
    assert_eq!(parsed.elements.len(), 2, "edges are not elements");
    let more = read("@startuml\nparticipant A\nparticipant B\nA -\\\\ B\nA -// B\nA x<- B\nA <<- B\n[o-> A\nA ->?\nA ->x]\n@enduml\n");
    assert!(more.unknown.is_empty(), "{:?}", more.unknown);
    let heads: Vec<(&str, &str, Option<&str>)> = more.links.iter().map(|l| (l.from.as_str(), l.to.as_str(), l.head)).collect();
    assert_eq!(heads, [("A", "B", Some("half")), ("A", "B", Some("half")), ("B", "A", Some("lost")), ("B", "A", Some("async")),
        ("[", "A", None), ("A", "]", None), ("A", "]", Some("lost"))]);
    // Heads mean nothing outside a sequence diagram; names may hold a slash.
    let plain = read("@startuml\n[A] ->> [B]\nrectangle a/b\n@enduml\n");
    assert_eq!(plain.links[0].head, None);
    assert!(plain.element("a/b").is_some());
    // A component written `[-x]` is still a component.
    assert_eq!(read("@startuml\n[-x]\n@enduml\n").elements[0].name, "-x");
    // Serialized, the edge end shows as its key and the flag.
    let json = serde_json::to_value(&parsed.links[7]).unwrap();
    assert_eq!(json["from"], "[");
    assert_eq!(json["boundary"], true);
}

#[test]
fn work_breakdowns_turn_branches_and_edit_like_mind_maps() {
    let text = "@startwbs\n* 项目\n**< 设计 <<done>>\n***< 草图\n**> 实现\n*** 编码\n@endwbs\n";
    let parsed = read(text);
    assert_eq!(parsed.status, Status::Ok, "{:?}", parsed.problems);
    let tree: Vec<(&str, Option<&str>)> = parsed.elements.iter().map(|e| (e.name.as_str(), e.parent.as_deref())).collect();
    assert_eq!(tree, [("项目", None), ("设计", Some("项目")), ("草图", Some("设计")), ("实现", Some("项目")), ("编码", Some("实现"))]);
    assert_eq!(parsed.element("设计").unwrap().stereotypes, ["<<done>>"]);
    let added = edit(text, Edit::Add { title: "测试", alias: None, parent: Some("实现"), token: None }).unwrap();
    assert!(added.contains("*** 编码\n*** 测试\n@endwbs"), "{added}");
    let state = edit(text, Edit::State { key: "实现", token: Some("<<wip>>"), remove: &[] }).unwrap();
    assert!(state.contains("**> 实现 <<wip>>\n"), "{state}");
    assert_eq!(changed_lines(text, &state), 1);
    let renamed = edit(text, Edit::Title { key: "草图", title: "线框" }).unwrap();
    assert!(renamed.contains("***< 线框\n"), "{renamed}");
    assert_eq!(edit(text, Edit::Link { from: "设计", to: "实现", label: None, dashed: false }).unwrap_err(), "@startwbs 图不能加连线或旁注");
}

const GANTT: &str = "@startgantt
' 发布计划
printscale daily
Project starts 2026-11-02
saturday are closed
-- 第一阶段 --
[设计] as [D] lasts 5 days and is 100% completed
[实现] as [I] lasts 2 weeks
[I] starts at [D]'s end
[I] is 40% complete
then [测试] lasts 3 days
[发布] happens at [测试]'s end
[文档] starts 2026-11-09 and ends 2026-11-13
[文档] -> [培训]
[培训] lasts 1 day and is colored in Lavender/LightBlue
[Ops] on {Alice} {Bob:50%} lasts 2 days
[A] starts at [B]'s end
[B] starts at [A]'s end
[X] lasts 4 days and pauses on 2026-11-05
[Y] starts 3 days after [D]'s end
@endgantt
";

#[test]
fn a_gantt_chart_reads_tasks_in_days_from_the_project_start() {
    let parsed = read(GANTT);
    assert_eq!(parsed.status, Status::Ok, "{:?}", parsed.problems);
    assert!(parsed.diagrams[0].supported);
    assert_eq!(parsed.diagrams[0].origin.as_deref(), Some("2026-11-02"));
    let time = |key: &str| {
        let element = parsed.element(key).unwrap_or_else(|| panic!("{key}"));
        let time = element.time.clone().unwrap();
        (element.keyword.as_str(), time.start, time.days, time.completed)
    };
    assert_eq!(time("D"), ("task", Some(0), Some(5), Some(100)));
    assert_eq!(time("I"), ("task", Some(5), Some(14), Some(40)));
    assert_eq!(time("测试"), ("task", Some(19), Some(3), None));
    assert_eq!(time("发布"), ("milestone", Some(22), Some(0), None));
    assert!(parsed.element("发布").unwrap().time.as_ref().unwrap().milestone);
    assert_eq!(time("文档"), ("task", Some(7), Some(5), None));
    assert_eq!(time("培训"), ("task", Some(12), Some(1), None));
    assert_eq!(time("Ops"), ("task", Some(0), Some(2), None));
    assert_eq!(time("A").1, None, "a cycle has no start");
    assert_eq!(parsed.element("I").unwrap().name, "实现");
    assert_eq!(parsed.element("D").unwrap().line, 7);
    let links: Vec<(&str, &str, usize)> = parsed.links.iter().map(|l| (l.from.as_str(), l.to.as_str(), l.line)).collect();
    assert_eq!(links, [("D", "I", 9), ("I", "测试", 11), ("测试", "发布", 12), ("文档", "培训", 14), ("B", "A", 17), ("A", "B", 18)]);
    let unknown: Vec<(usize, &str)> = parsed.unknown.iter().map(|line| (line.line, line.reason.as_str())).collect();
    assert_eq!(unknown, [(5, "画布按日历天排，不扣除休息日"), (19, "不认识的甘特图写法"), (20, "不认识的甘特图写法")]);
    // Kept as written: an unknown line survives an edit byte for byte.
    let after = edit(GANTT, Edit::Add { title: "上线", alias: None, parent: None, token: None }).unwrap();
    assert!(after.starts_with(&GANTT[..GANTT.len() - "@endgantt\n".len()]));
    assert!(after.ends_with("[Y] starts 3 days after [D]'s end\n[上线] as [E1] lasts 1 day\n@endgantt\n"), "{after}");
    // Without a project start, the earliest date is the origin; without any, none.
    let dated = read("@startgantt\n[a] starts 2026-01-05 and lasts 2 days\n[b] starts 2026-01-03 and lasts 1 day\n@endgantt\n");
    assert_eq!(dated.diagrams[0].origin.as_deref(), Some("2026-01-03"));
    let bare = read("@startgantt\n[Task] lasts 5 days\n@endgantt\n");
    assert_eq!(bare.diagrams[0].origin, None);
    assert_eq!(bare.element("Task").unwrap().time.as_ref().unwrap().start, Some(0));
    assert_eq!(read("@startgantt\nProject starts the 1st of may\n@endgantt\n").unknown[0].reason, "甘特图日期只认 YYYY-MM-DD");
}

#[test]
fn a_long_gantt_chain_does_not_overflow() {
    let mut text = String::from("@startgantt\n[T0] lasts 1 day\n");
    for n in 1..20_000 {
        text.push_str(&format!("then [T{n}] lasts 1 day\n"));
    }
    text.push_str("@endgantt\n");
    let parsed = read(&text);
    assert_eq!(parsed.element("T19999").unwrap().time.as_ref().unwrap().start, Some(19_999));
}

#[test]
fn the_canvas_edits_a_gantt_chart_only_where_it_is_safe() {
    let text = "@startgantt\n[设计] as [D] lasts 5 days\n[实现] lasts 3 days\n[联调] lasts 2 days\n[联调] starts at [实现]'s end\n@endgantt\n";
    let linked = edit(text, Edit::Link { from: "D", to: "实现", label: None, dashed: false }).unwrap();
    assert!(linked.ends_with("[实现] starts at [D]'s end\n@endgantt\n"), "{linked}");
    assert_eq!(read(&linked).element("联调").unwrap().time.as_ref().unwrap().start, Some(8));
    for (change, reason) in [
        (Edit::Link { from: "实现", to: "联调", label: None, dashed: false }, "任务 联调 已有开始日期或前置任务；在文件里改".to_owned()),
        (Edit::Link { from: "D", to: "实现", label: None, dashed: true }, "甘特图的依赖没有虚线".to_owned()),
        (Edit::Link { from: "D", to: "实现", label: Some("先"), dashed: false }, "甘特图的依赖不带标签".to_owned()),
        (Edit::State { key: "D", token: Some("<<done>>"), remove: &[] }, GANTT_STATE.to_owned()),
        (Edit::Note { key: "D", token: None, text: "理由" }, "甘特图不能加旁注".to_owned()),
        (Edit::Add { title: "子", alias: None, parent: Some("D"), token: None }, "甘特图的任务没有上级".to_owned()),
        (Edit::Add { title: "子", alias: None, parent: None, token: Some("<<done>>") }, GANTT_STATE.to_owned()),
        (Edit::Add { title: "a]b", alias: None, parent: None, token: None }, "任务名须非空、单行，且不含方括号".to_owned()),
        (Edit::Title { key: "实现", title: "开发" }, "任务 实现 在第 5 行也被提到，改名会断开那里的引用；先在文件里给它写 as [别名]，别处改用别名".to_owned()),
    ] {
        assert_eq!(edit(text, change.clone()).unwrap_err(), reason, "{change:?}");
    }
    // A dependency that would close a cycle is refused.
    assert_eq!(edit(&linked, Edit::Link { from: "联调", to: "D", label: None, dashed: false }).unwrap_err(), "这条依赖会成环：联调 已排在 D 之后");
    let renamed = edit(text, Edit::Title { key: "D", title: "方案" }).unwrap();
    assert_eq!(changed_lines(text, &renamed), 1);
    assert!(renamed.contains("[方案] as [D] lasts 5 days\n"));
    let plain = "@startgantt\n[设计] lasts 5 days\n@endgantt\n";
    let renamed = edit(plain, Edit::Title { key: "设计", title: "方案" }).unwrap();
    assert_eq!(renamed, "@startgantt\n[方案] lasts 5 days\n@endgantt\n");
    let added = edit(plain, Edit::Add { title: "评审", alias: Some("R"), parent: None, token: None }).unwrap();
    assert_eq!(read(&added).element("R").unwrap().time.as_ref().unwrap().days, Some(1));
    // CRLF and the bytes around the edit stay.
    let crlf = plain.replace('\n', "\r\n");
    let added = edit(&crlf, Edit::Add { title: "评审", alias: None, parent: None, token: None }).unwrap();
    assert_eq!(added, "@startgantt\r\n[设计] lasts 5 days\r\n[评审] as [E1] lasts 1 day\r\n@endgantt\r\n");
}
