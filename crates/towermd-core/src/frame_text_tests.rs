use super::*;
use crate::{
    cards::View,
    frames::{self, RulesInput, Template, CREATED},
    hub::Event,
};
use serde_json::json;

fn kanban() -> Value {
    json!({"id":"kanban","version":"1.0.0","type":"template","title":"看板",
        "layout":{"x":{"category":"state"},"y":{"order":"added"},"structure":"none"},
        "states":["待办","进行中","待评审","完成"],"transitions":{"待评审":["完成","进行中"]},"done_states":["完成"],
        "card_fields":{"owner":"text","due":"date","source":"ref"},"limits":{"title_max":30,"body_max":100,"per_lane_max":7},
        "default_set_by":"delegate","export":["mermaid","markdown"]})
}

fn mindmap() -> Value {
    json!({"id":"mindmap","version":"1.0.0","type":"template","title":"思维导图",
        "layout":{"x":"none","y":"none","structure":"tree"},"card_fields":{"source":"ref"},
        "limits":{"title_max":30,"body_max":100},"default_set_by":"human","export":["mermaid"]})
}

fn quadrant() -> Value {
    json!({"id":"quadrant","version":"1.0.0","type":"template","title":"四象限",
        "layout":{"x":{"category":"urgency","values":["紧急","不紧急"]},"y":{"category":"importance","values":["重要","不重要"]},"structure":"none"},
        "card_fields":{"urgency":"text","importance":"text","source":"ref"},"limits":{"title_max":30,"body_max":100,"per_lane_max":7},
        "default_set_by":"human","export":["markdown"]})
}

fn timeline() -> Value {
    json!({"id":"timeline","version":"1.0.0","type":"template","title":"时间线",
        "layout":{"x":{"time":"date"},"y":"none","structure":"none"},"card_fields":{"date":"date","source":"ref"},
        "limits":{"title_max":30,"body_max":100},"default_set_by":"delegate","export":["mermaid"]})
}

fn graph() -> Value {
    json!({"id":"graph","version":"1","type":"template","layout":{"structure":["relations","nesting"]},
        "card_fields":{"source":"ref"},"default_set_by":"human"})
}

/// Imports `text` into a new free frame `C-new` made with `definition`,
/// through the one `frame.created` event, and reads the frame back.
fn import(definition: Value, text: &str) -> Result<View, Vec<String>> {
    let template = Template::parse(&definition).unwrap();
    let fields: Vec<String> = template.card_fields.keys().cloned().collect();
    let parsed = parse(text)?;
    let cards = writes(&parsed, "C-new", &template, &fields)?;
    let data = json!({"title": parsed.title.clone().unwrap_or("导入".into()),
        "template":{"id":template.id,"version":template.version,"definition":definition},
        "set_by":"human","mode":"free","rules":template.rules(RulesInput::default()).unwrap(),
        "place":{"x":0,"y":0,"width":880,"height":420},"cards":cards});
    let event: Event = serde_json::from_value(json!({"seq":1,"eid":"e1","host":"h","t":"2026-10-24T01:00:00Z",
        "actor":"human:local","type":CREATED,"layer":frames::LAYER,"obj":"C-new","data":data,"guards":[],
        "epoch":null,"stale":false,"cost_usd":null})).unwrap();
    let mut state = json!({});
    frames::apply(&mut state, &event, &[]).map_err(|error| vec![error])?;
    let frame = frames::read(&state).unwrap()["C-new"].clone();
    Ok(View { frame, bound: Vec::new(), frozen: false, callouts: Vec::new() })
}

fn titles(view: &View) -> Vec<(String, Option<String>, Option<String>)> {
    view.frame.cards.iter().map(|card| (card.title.clone(), card.state.clone(), card.parent.clone())).collect()
}

#[test]
fn a_kanban_list_lands_in_its_lanes_and_exports_back_to_the_same_text() {
    let text = "# 发布\n\n## 待办\n\n- 写说明\n  先写中文\n  <!-- fields {\"owner\":\"amy\"} -->\n- 打标签\n\n## 完成\n\n- 冻结分支\n";
    let view = import(kanban(), text).unwrap();
    assert_eq!(view.frame.title, "发布");
    assert_eq!(titles(&view), vec![
        ("写说明".into(), Some("待办".into()), None),
        ("打标签".into(), Some("待办".into()), None),
        ("冻结分支".into(), Some("完成".into()), None),
    ]);
    let first = &view.frame.cards[0];
    assert_eq!((first.id.as_str(), first.body.as_str(), &first.fields["owner"]), ("C-new.1", "先写中文", &json!("amy")));
    assert!(first.human.contains("title"), "imported cards are the person's own writes");
    assert_eq!(export(&view, MARKDOWN).unwrap(), text);
    let mermaid = export(&view, MERMAID).unwrap();
    assert_eq!(mermaid, "kanban\n  lane1[待办]\n    n1[写说明]\n    n2[打标签]\n  lane2[进行中]\n  lane3[待评审]\n  lane4[完成]\n    n3[冻结分支]\n");
    let again = import(kanban(), &mermaid).unwrap();
    assert_eq!(titles(&again), titles(&view));
}

#[test]
fn a_mindmap_keeps_its_tree_both_ways() {
    let text = "mindmap\n  root((发布计划))\n    文档\n      中文说明\n      英文说明\n    打包[\"签名#58; macOS\"]\n";
    let view = import(mindmap(), text).unwrap();
    assert_eq!(view.frame.title, "发布计划");
    assert_eq!(titles(&view), vec![
        ("文档".into(), None, None),
        ("中文说明".into(), None, Some("C-new.1".into())),
        ("英文说明".into(), None, Some("C-new.1".into())),
        ("签名: macOS".into(), None, None),
    ]);
    let mermaid = export(&view, MERMAID).unwrap();
    assert_eq!(mermaid, "mindmap\n  root[发布计划]\n    n1[文档]\n      n2[中文说明]\n      n3[英文说明]\n    n4[签名#58; macOS]\n");
    assert_eq!(titles(&import(mindmap(), &mermaid).unwrap()), titles(&view));
    let markdown = export(&view, MARKDOWN).unwrap();
    assert_eq!(markdown, "# 发布计划\n\n- 文档\n  - 中文说明\n  - 英文说明\n- 签名: macOS\n");
    assert_eq!(titles(&import(mindmap(), &markdown).unwrap()), titles(&view));
}

#[test]
fn two_category_axes_read_and_write_as_a_table() {
    let text = "# 本周\n\n| | 紧急 | 不紧急 |\n| --- | --- | --- |\n| 重要 | 修崩溃<br>回邮件 | 写规格 |\n| 不重要 |  | 整理书签 |\n";
    let view = import(quadrant(), text).unwrap();
    let placed: Vec<(String, Value, Value)> = view.frame.cards.iter()
        .map(|card| (card.title.clone(), card.fields["urgency"].clone(), card.fields["importance"].clone())).collect();
    assert_eq!(placed, vec![
        ("修崩溃".into(), json!("紧急"), json!("重要")),
        ("回邮件".into(), json!("紧急"), json!("重要")),
        ("写规格".into(), json!("不紧急"), json!("重要")),
        ("整理书签".into(), json!("不紧急"), json!("不重要")),
    ]);
    assert_eq!(export(&view, MARKDOWN).unwrap(), text);
    assert_eq!(formats(&view.frame.layout, &["markdown".into()]), vec![MARKDOWN], "two category axes have no Mermaid form");
    assert!(export(&view, MERMAID).unwrap_err().contains("不能导出为 mermaid"));
    let heads = "## 紧急\n\n### 重要\n\n- 修崩溃\n";
    assert_eq!(import(quadrant(), heads).unwrap().frame.cards[0].fields["importance"], json!("重要"));
}

#[test]
fn a_timeline_places_cards_on_dates() {
    let text = "timeline\n  title 版本\n  2026-10-01 : 冻结 : 打标签\n  2026-10-20 : 发布\n";
    let view = import(timeline(), text).unwrap();
    let dates: Vec<(String, Value)> = view.frame.cards.iter().map(|card| (card.title.clone(), card.fields["date"].clone())).collect();
    assert_eq!(dates, vec![("冻结".into(), json!("2026-10-01")), ("打标签".into(), json!("2026-10-01")), ("发布".into(), json!("2026-10-20"))]);
    assert_eq!(export(&view, MERMAID).unwrap(), text);
    let markdown = export(&view, MARKDOWN).unwrap();
    assert_eq!(markdown, "# 版本\n\n## 2026-10-01\n\n- 冻结\n- 打标签\n\n## 2026-10-20\n\n- 发布\n");
    assert_eq!(import(timeline(), &markdown).unwrap().frame.cards.len(), 3);
}

#[test]
fn a_flowchart_brings_its_groups_and_lines() {
    let text = "flowchart LR\n  subgraph g1[\"后端\"]\n    a[\"守护进程\"]\n    b(索引)\n  end\n  a --> b\n  b -->|读| c{画布}\n";
    let view = import(graph(), text).unwrap();
    assert_eq!(titles(&view), vec![
        ("后端".into(), None, None),
        ("守护进程".into(), None, Some("C-new.1".into())),
        ("索引".into(), None, Some("C-new.1".into())),
        ("画布".into(), None, None),
    ]);
    assert_eq!(view.frame.cards[1].links, vec!["C-new.3"]);
    assert_eq!(view.frame.cards[2].links, vec!["C-new.4"]);
    let mermaid = export(&view, MERMAID).unwrap();
    assert!(mermaid.contains("subgraph n1[\"后端\"]\n    n2[\"守护进程\"]\n    n3[\"索引\"]\n  end\n  n4[\"画布\"]\n  n2 --> n3\n  n3 --> n4\n"), "{mermaid}");
    let again = import(graph(), &mermaid).unwrap();
    assert_eq!(again.frame.title, "导入");
    assert_eq!(titles(&again), titles(&view));
    let markdown = export(&view, MARKDOWN).unwrap();
    assert!(markdown.contains("  - 守护进程\n    → 索引\n"), "{markdown}");
    let back = import(graph(), &markdown).unwrap();
    assert_eq!(back.frame.cards[1].links, vec!["C-new.3"]);
}

#[test]
fn what_does_not_fit_is_reported_by_line_and_nothing_is_created() {
    let problems = import(kanban(), "## 待办\n- 写说明\n\n## 搁置\n- 翻旧账\n").unwrap_err();
    assert_eq!(problems, vec!["第 5 行：「搁置」不是模板「看板」的状态（待办、进行中、待评审、完成）"]);
    let problems = import(timeline(), "## 下周\n- 发布\n").unwrap_err();
    assert_eq!(problems, vec!["第 2 行：「下周」不是 YYYY-MM-DD 日期，排不上时间轴"]);
    let problems = import(timeline(), "- 发布\n").unwrap_err();
    assert_eq!(problems, vec!["第 1 行：卡片没有 date，它决定卡片在模板「时间线」里的位置"]);
    let problems = import(kanban(), "- 文档\n  - 中文\n").unwrap_err();
    assert_eq!(problems, vec!["第 2 行：模板「看板」的结构不是嵌套或树，放不下上下级"]);
    let problems = import(mindmap(), "flowchart\n  a --> b\n").unwrap_err();
    assert_eq!(problems, vec!["第 2 行：模板「思维导图」没有关系线，放不下 1 条连线"]);
    let problems = import(quadrant(), "| | 紧急 |\n|---|---|\n| 重要 | 修崩溃 |\n| 顺手 | 擦桌子 |\n").unwrap_err();
    assert_eq!(problems, vec!["第 4 行：importance 只能取 重要、不重要，不能是「顺手」"]);
    let long = "长".repeat(31);
    assert_eq!(import(mindmap(), &format!("- {long}\n")).unwrap_err(),
        vec![format!("第 1 行：卡片标题须有 1 到 30 字，「{long}」有 31 字")]);
    assert_eq!(parse("- 一\n随手一句\n").unwrap_err(), vec!["第 2 行：不是列表项、标题或表格"]);
    assert_eq!(parse("- 一\n  → 二\n").unwrap_err(), vec!["第 2 行：连线指向的「二」不存在"]);
    assert!(parse("@startuml\nA -> B\n@enduml\n").unwrap_err()[0].contains("PlantUML"));
    assert!(parse("sequenceDiagram\nA->>B: hi\n").unwrap_err()[0].contains("sequenceDiagram 暂不能导入"));
    assert_eq!(parse("\n\n").unwrap_err(), vec!["没有读出任何卡片"]);
    let many: String = (0..=IMPORT_MAX).map(|n| format!("- 卡{n}\n")).collect();
    assert!(parse(&many).unwrap_err()[0].contains("一次最多导入 200 项"));
}

#[test]
fn formats_put_the_declared_ones_first_and_markdown_always_fits() {
    let layout = |definition: Value| Template::parse(&definition).unwrap().layout;
    assert_eq!(formats(&layout(kanban()), &["mermaid".into(), "markdown".into()]), vec![MERMAID, MARKDOWN]);
    assert_eq!(formats(&layout(mindmap()), &[]), vec![MARKDOWN, MERMAID]);
    assert_eq!(formats(&layout(quadrant()), &["mermaid".into()]), vec![MARKDOWN]);
    assert_eq!(formats(&layout(timeline()), &["gantt".into()]), vec![MARKDOWN, MERMAID]);
}

#[test]
fn bound_cards_export_with_their_state_and_archived_cards_do_not() {
    let mut view = import(kanban(), "## 待办\n- 写说明\n- 旧事\n").unwrap();
    view.frame.cards[1].archived = true;
    view.bound.push(crate::cards::Bound { id: "T-1".into(), title: "修登录".into(), state: "进行中".into(),
        phase: crate::delegation::Phase::Running, done: false });
    assert_eq!(export(&view, MARKDOWN).unwrap(), "# 导入\n\n## 待办\n\n- 写说明\n\n## 进行中\n\n- 修登录\n");
}
