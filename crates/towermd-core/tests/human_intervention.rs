use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};
use towermd_core::{
    hub::{AppendReceipt, Event, Fence, Hub},
    hub_protocol::{AcquireLease, HubProtocol, NewEvent},
    human_intervention::*,
    recovery_contract::recorded::{
        request_event_eid, RequestBody, RequestMeta, RequestReceipt, ReviewBodyView,
        ReviewHoldView, ReviewReplyView, TaskReviewAction, TaskReviewRecorded, TaskReviewResult,
    },
    snapshot::{Snapshot, StateProjection},
};
fn recorded(receipt: AppendReceipt) -> Event {
    match receipt {
        AppendReceipt::Event(event) => *event,
        AppendReceipt::Duplicate { .. } => panic!("actual Event required"),
    }
}
fn hash(value: &impl serde::Serialize) -> String {
    fn sorted(value: Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut entries = map.into_iter().collect::<Vec<_>>();
                entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
                Value::Object(
                    entries
                        .into_iter()
                        .map(|(key, value)| (key, sorted(value)))
                        .collect(),
                )
            }
            Value::Array(values) => Value::Array(values.into_iter().map(sorted).collect()),
            other => other,
        }
    }
    hex::encode(Sha256::digest(
        serde_json::to_vec(&sorted(serde_json::to_value(value).unwrap())).unwrap(),
    ))
}
struct Fixture {
    temp: tempfile::TempDir,
    hub: Hub,
    fences: Vec<Fence>,
    main_epoch: u64,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut hub = Hub::open(temp.path()).unwrap();
        let main = hub
            .lease_acquire(
                AcquireLease {
                    host: "host".into(),
                    holder: "writer".into(),
                    names: vec!["main".into()],
                    ttl_ms: 1_000_000,
                },
                1,
            )
            .unwrap();
        let module = hub
            .lease_acquire(
                AcquireLease {
                    host: "host".into(),
                    holder: "R".into(),
                    names: vec!["module:A".into(), "module:B".into()],
                    ttl_ms: 1_000_000,
                },
                2,
            )
            .unwrap();
        Self {
            temp,
            hub,
            fences: module.leases.iter().map(|lease| lease.fence()).collect(),
            main_epoch: main.leases[0].epoch,
        }
    }
    fn open(&mut self, text: &str, raw_seq: u64) -> (Event, ReviewBinding) {
        let binding = ReviewBinding {
            run: "R".into(),
            task: "T".into(),
            baseline: "towermd/B0".into(),
            fences: self.fences.clone(),
            subject: ReviewSubject::Plan {
                sha256: hex::encode(Sha256::digest(text.as_bytes())),
            },
        };
        let hold = ReviewHoldView {
            object_type: "Hold".into(),
            id: format!("H-{}", uuid::Uuid::now_v7()),
            body: ReviewBodyView::Review {
                binding: binding.clone(),
                verdict: None,
                plan_content: Some(text.into()),
                source_commit: None,
                source_path: Some("uncommitted-plan.md".into()),
                comment: None,
            },
            opened_ms: 10,
            opened_seq: raw_seq,
            reply: None,
            extra: BTreeMap::from([
                ("future_hold".into(), json!({"keep":[1,null]})),
                ("waits".into(), json!([])),
                (
                    "recipients".into(),
                    json!([{"run":"R","task":"T","host":"host","worktree":"/fixture","branch":"t/T","fences":self.fences}]),
                ),
                ("delivered".into(), json!({})),
                ("attention".into(), json!(false)),
            ]),
        };
        let event = recorded(
            self.hub
                .append(
                    NewEvent {
                        eid: uuid::Uuid::now_v7().to_string(),
                        host: "host".into(),
                        actor: "process:towermd-cli".into(),
                        kind: "hold.opened".into(),
                        layer: "L5".into(),
                        obj: hold.id.clone(),
                        data: json!({"hold":hold}),
                        guards: self.fences.clone(),
                    },
                    10,
                )
                .unwrap(),
        );
        (event, binding)
    }
    fn answer(&mut self, opening: &Event, verdict: ReviewVerdict) -> Event {
        let mut hold: ReviewHoldView =
            serde_json::from_value(opening.data["hold"].clone()).unwrap();
        let binding = match &hold.body {
            ReviewBodyView::Review { binding, .. } => binding.clone(),
        };
        let request = uuid::Uuid::now_v7().to_string();
        let eid = request_event_eid(&request, "hold.reviewed").unwrap();
        let comment = "human original answer".to_string();
        let actor = "human:fixture".to_string();
        let fact = ReviewFact {
            binding: binding.clone(),
            verdict: verdict.clone(),
            actor: actor.clone(),
            answer_eid: eid.clone(),
        };
        let ReviewBodyView::Review {
            verdict: outcome,
            comment: text,
            ..
        } = &mut hold.body;
        *outcome = Some(verdict.clone());
        *text = Some(comment.clone());
        hold.reply = Some(ReviewReplyView {
            id: format!("reply-{request}"),
            text: comment.clone(),
            actor: actor.clone(),
            at_ms: 20,
            extra: BTreeMap::from([("future_reply".into(), json!(["keep"]))]),
        });
        let body = RequestBody {
            project_id: "project".into(),
            host: "host".into(),
            actor: actor.clone(),
            // App 回答用 action.binding 定位运行，原请求外层 run 必须显式为 null。
            run: None,
            action: TaskReviewAction::Review {
                hold: hold.id.clone(),
                binding,
                verdict: verdict.clone(),
                comment: comment.clone(),
            },
        };
        let meta = RequestMeta {
            request_id: request,
            body_hash: hash(&body),
        };
        let result = TaskReviewResult {
            hold: hold.id.clone(),
            fact: fact.clone(),
            verdict,
            comment: comment.clone(),
        };
        let data = TaskReviewRecorded {
            hold: hold.clone(),
            fact,
            comment,
            request_body: body,
            p3_request: meta.clone(),
            p3_receipt: RequestReceipt {
                request_id: meta.request_id,
                body_hash: meta.body_hash,
                result_ref: result,
            },
        };
        recorded(
            self.hub
                .append(
                    NewEvent {
                        eid,
                        host: "host".into(),
                        actor,
                        kind: "hold.reviewed".into(),
                        layer: "L5".into(),
                        obj: hold.id,
                        data: json!(data),
                        guards: self.fences.clone(),
                    },
                    20,
                )
                .unwrap(),
        )
    }
    fn events(&mut self) -> Vec<Event> {
        self.hub.subscribe(0, 30).unwrap().events
    }
}
#[test]
fn actual_hub_review_requires_exact_human_binding_and_preserves_raw_order() {
    let mut f = Fixture::new();
    let (open, binding) = f.open("plan original", 999);
    let answer = f.answer(&open, ReviewVerdict::Approved);
    let mut state = HumanState::default();
    for event in f.events() {
        state.apply(&event).unwrap();
    }
    assert_eq!(state.holds[&open.obj].opened_seq, open.seq);
    assert_ne!(open.seq, 999);
    assert_eq!(state.current_plan_subject("R").unwrap(), binding.subject);
    assert_eq!(
        state.require_human_review(&binding).unwrap().answer_eid,
        answer.eid
    );
    assert_eq!(
        state.original_review_hold(&open.obj).unwrap().opened_seq,
        999
    );
    assert_eq!(state.review_originals(&binding).unwrap(), (&open, &answer));
    assert_eq!(answer.data["hold"]["opened_seq"], 999);
    let mut reordered = binding.clone();
    reordered.fences.reverse();
    assert!(state.require_human_review(&reordered).is_ok());
    for change in ["epoch", "run", "task", "baseline", "subject", "duplicate"] {
        let mut changed = binding.clone();
        match change {
            "epoch" => changed.fences[1].epoch += 1,
            "run" => changed.run = "other".into(),
            "task" => changed.task = "other".into(),
            "baseline" => changed.baseline = "towermd/B1".into(),
            "subject" => {
                changed.subject = ReviewSubject::Tree {
                    tree: "a".repeat(40),
                }
            }
            _ => changed.fences.push(changed.fences[0].clone()),
        };
        assert!(state.require_human_review(&changed).is_err(), "{change}");
    }
    let (holds, canonical) = state.projection();
    let mut projection = StateProjection::default();
    projection.objects.insert("Hold".into(), holds.clone());
    let mut restored = HumanState::default();
    restored.restore_projected(&projection, canonical).unwrap();
    assert_eq!(
        restored.review_originals(&binding).unwrap(),
        (&open, &answer)
    );
    drop(f.hub);
    let mut reopened = Hub::open(f.temp.path()).unwrap();
    let mut replay = HumanState::default();
    for event in reopened.subscribe(0, 30).unwrap().events {
        replay.apply(&event).unwrap();
    }
    assert_eq!(replay.review_originals(&binding).unwrap(), (&open, &answer));
}
#[test]
fn latest_actual_plan_opening_cannot_reuse_an_old_approval_even_when_hash_returns() {
    let mut f = Fixture::new();
    let (first, binding) = f.open("plan A", 999);
    f.answer(&first, ReviewVerdict::Approved);
    let (_, second) = f.open("plan B", 0);
    let (third, third_binding) = f.open("plan A", 0);
    let mut state = HumanState::default();
    for event in f.events() {
        state.apply(&event).unwrap();
    }
    assert_ne!(second.subject, binding.subject);
    assert_eq!(third_binding.subject, binding.subject);
    assert_eq!(
        state.current_plan_subject("R").unwrap(),
        third_binding.subject
    );
    assert!(state.require_human_review(&binding).is_err());
    assert_eq!(state.holds[&third.obj].opened_seq, third.seq);
}
#[test]
fn review_answer_keeps_runtime_waits_without_changing_original_proof_hold() {
    let mut f = Fixture::new();
    let (open, binding) = f.open("plan with waits", 0);
    let mut state = HumanState::default();
    state.apply(&open).unwrap();
    let mut hold = open.data["hold"].clone();
    hold["waits"] = json!([{"id":uuid::Uuid::now_v7().to_string(),"recipient_run":"R","started_ms":10,"deadline_ms":90010,"finished_ms":null}]);
    let waited = recorded(
        f.hub
            .append(
                NewEvent {
                    eid: uuid::Uuid::now_v7().to_string(),
                    host: "host".into(),
                    actor: "process:towermd-cli".into(),
                    kind: "hold.wait_started".into(),
                    layer: "L5".into(),
                    obj: open.obj.clone(),
                    data: json!({"hold":hold}),
                    guards: f.fences.clone(),
                },
                15,
            )
            .unwrap(),
    );
    state.apply(&waited).unwrap();
    let answer = f.answer(&open, ReviewVerdict::Approved);
    state.apply(&answer).unwrap();
    assert_eq!(state.holds[&open.obj].waits.len(), 1);
    assert_eq!(answer.data["hold"]["waits"], json!([]));
    assert_eq!(state.original_review_hold(&open.obj).unwrap().opened_seq, 0);
    assert!(state.require_human_review(&binding).is_ok());
}
#[test]
fn rejected_stale_or_mutated_original_answers_never_authorize() {
    for verdict in [ReviewVerdict::Rejected, ReviewVerdict::ChangesRequested] {
        let mut f = Fixture::new();
        let (open, binding) = f.open("plan", 0);
        let answer = f.answer(&open, verdict);
        let mut state = HumanState::default();
        state.apply(&open).unwrap();
        state.apply(&answer).unwrap();
        assert!(state.require_human_review(&binding).is_err());
    }
    let mut f = Fixture::new();
    let (open, binding) = f.open("plan", 0);
    let answer = f.answer(&open, ReviewVerdict::Approved);
    let mut state = HumanState::default();
    state.apply(&open).unwrap();
    let mut stale = answer.clone();
    stale.stale = true;
    state.apply(&stale).unwrap();
    assert!(state.require_human_review(&binding).is_err());
    for field in ["opened_seq", "future_hold"] {
        let mut changed = answer.clone();
        changed.data["hold"][field] = json!("changed");
        assert!(state.apply(&changed).is_err());
        assert!(state.require_human_review(&binding).is_err());
    }
    let mut cli_binding = answer.clone();
    cli_binding.data["request_body"]["run"] = json!("R");
    let cli_hash = hash(&cli_binding.data["request_body"]);
    cli_binding.data["p3_request"]["body_hash"] = json!(&cli_hash);
    cli_binding.data["p3_receipt"]["body_hash"] = json!(&cli_hash);
    assert_eq!(
        state.apply(&cli_binding).unwrap_err(),
        "original typed request/receipt mismatch"
    );
    assert!(state.require_human_review(&binding).is_err());
    state.apply(&answer).unwrap();
    let rebuilt = recorded(
        f.hub
            .append(
                NewEvent {
                    eid: uuid::Uuid::now_v7().to_string(),
                    host: "host".into(),
                    actor: "process:towermd-daemon".into(),
                    kind: "hub.rebuilt".into(),
                    layer: "L3".into(),
                    obj: "main".into(),
                    data: json!({}),
                    guards: vec![],
                },
                25,
            )
            .unwrap(),
    );
    state.apply(&rebuilt).unwrap();
    assert!(state.current_plan_subject("R").is_err());
    assert!(state.require_human_review(&binding).is_err());
    assert!(state.holds[&open.obj].reply.is_some());
}
#[test]
fn actual_snapshot_restores_review_fact_without_reinterpreting_raw_seq() {
    let mut f = Fixture::new();
    let (open, binding) = f.open("snapshot original plan", 999);
    f.answer(&open, ReviewVerdict::Approved);
    let events = f.events();
    let snapshot =
        Snapshot::from_events("project", &events, events.last().unwrap().seq, f.main_epoch)
            .unwrap();
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    let decoded: Snapshot = serde_json::from_slice(&bytes).unwrap();
    let mut state = HumanState::default();
    state.restore(&decoded.projection).unwrap();
    assert!(
        state.require_human_review(&binding).is_ok(),
        "actual snapshot must retain canonical review provenance, not only in-memory facts"
    );
}
#[test]
fn normalization_is_exact_and_module_scoped() {
    assert_eq!(normalize_question("  ＡＢＣ\t\n 写入？！  "), "abc 写入");
    assert_eq!(normalize_question("ÄBC + 队列"), "Äbc + 队列");
    assert_eq!(normalize_question(" x—“?!"), "x");
    assert_eq!(fingerprint("A", "写入？"), fingerprint("A", "写入!"));
    for module in ["a", "B", "Ａ", " A"] {
        assert_ne!(fingerprint("A", "写入"), fingerprint(module, "写入"));
    }
    assert_ne!(fingerprint("A", "写入"), fingerprint("A", "写 入"));
    assert_eq!(normalize_question("x? !"), "x?");
    assert_eq!(normalize_question("x\u{061D}"), "x");
}
#[test]
fn normalization_uses_the_same_unicode17_punctuation_data_as_nfkc() {
    assert_eq!(unicode_normalization::UNICODE_VERSION, (17, 0, 0));
    // General_Category=P from official Unicode 17 UCD. These were introduced
    // in Unicode 14, 15, 16 and 17 and have no compatibility decompositions.
    for punctuation in [
        '\u{061D}',  // Arabic end of text mark, 14.0, Po.
        '\u{2E54}',  // Medieval question mark, 14.0, Po.
        '\u{10F86}', // Old Uyghur punctuation bar, 14.0, Po.
        '\u{11B00}', // Devanagari head mark, 15.0, Po.
        '\u{113D4}', // Tulu-Tigalari danda, 16.0, Po.
        '\u{10ED0}', // Arabic biblical end of verse, 17.0, Po.
    ] {
        let question = format!("  ＡＢＣ\t{punctuation}  ");
        assert_eq!(normalize_question(&question), "abc", "{punctuation:?}");
        assert_eq!(fingerprint("A", &question), fingerprint("A", "ABC!"));
        let interior = format!("x{punctuation}y");
        assert_eq!(normalize_question(&interior), interior);
    }
    // Sc, Sm, Sk and So are symbols, not General_Category=P.
    // U+1F6DD PLAYGROUND SLIDE was introduced in Unicode 14.0 (So).
    for symbol in ['$', '+', '^', '\u{1F6DD}'] {
        let question = format!("x{symbol}");
        assert_eq!(normalize_question(&question), question);
        assert_eq!(normalize_question(&format!("x{symbol}\u{10ED0}")), question);
        assert_ne!(fingerprint("A", &question), fingerprint("A", "x"));
    }
    // Keep the existing one-pass suffix rule; do not strip punctuation again
    // across the whitespace removed after the first punctuation suffix.
    assert_eq!(normalize_question("x? \u{10ED0}"), "x?");
}
fn ask() -> Hold {
    Hold {
        object_type: "Hold".into(),
        id: "H-ask".into(),
        body: HoldBody::Ask {
            fingerprint: fingerprint("A", "question"),
            question: "question".into(),
            module: "A".into(),
            options: vec![],
            recommended: None,
            context: String::new(),
        },
        recipients: vec![Recipient {
            run: "R".into(),
            task: "T".into(),
            host: "host".into(),
            worktree: "/fixture".into(),
            branch: "t/T".into(),
            fences: vec![Fence {
                name: "module:A".into(),
                holder: "R".into(),
                epoch: 7,
            }],
        }],
        opened_ms: 0,
        opened_seq: 3,
        reply: None,
        waits: vec![],
        delivered: BTreeMap::new(),
        attention: false,
        extra: BTreeMap::new(),
    }
}
#[test]
fn waits_are_capped_persisted_per_run_and_never_authorize() {
    let mut hold = ask();
    let request = uuid::Uuid::now_v7().to_string();
    let first = next_slice(&hold, "R", 0, &request).unwrap().unwrap();
    assert_eq!(first.id, request);
    assert_eq!(first.deadline_ms, 90_000);
    hold.waits.push(first);
    assert_eq!(
        next_slice(&hold, "R", 10, &request).unwrap(),
        Some(hold.waits[0].clone())
    );
    assert!(next_slice(&hold, "R", 1, &uuid::Uuid::now_v7().to_string())
        .unwrap()
        .is_none());
    assert_eq!(wait_budget(&hold, "R", 89_999), 89_999);
    assert_eq!(wait_budget(&hold, "R", 90_000), 90_000);
    hold.waits[0].finished_ms = Some(90_000);
    for start in [90_000, 180_000, 270_000, 360_000, 450_000, 540_000] {
        let mut slice = next_slice(&hold, "R", start, &uuid::Uuid::now_v7().to_string())
            .unwrap()
            .unwrap();
        slice.finished_ms = Some(slice.deadline_ms);
        hold.waits.push(slice);
    }
    assert_eq!(wait_budget(&hold, "R", 700_000), 600_000);
    assert_eq!(wait_budget(&hold, "another-run", 700_000), 0);
    assert!(
        next_slice(&hold, "R", 700_000, &uuid::Uuid::now_v7().to_string())
            .unwrap()
            .is_none()
    );
    assert!(hold.reply.is_none());
    let live = hold.recipients[0].fences.clone();
    assert_eq!(ask_state(&hold, "R", &live, 700_000), AskState::Deferred);
    let mut stale = live.clone();
    stale[0].epoch += 1;
    assert_eq!(ask_state(&hold, "R", &stale, 700_000), AskState::Expired);
    assert!(next_slice(&hold, "R", 0, "not-request-uuid").is_err());
    let restored: Hold = serde_json::from_value(serde_json::to_value(&hold).unwrap()).unwrap();
    assert_eq!(restored, hold);
    assert_eq!(wait_budget(&restored, "R", 700_000), 600_000);
}
fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn decision_catalog(
    decisions: &[(&str, &str)],
) -> (tempfile::TempDir, towermd_core::catalog::Catalog) {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-qb", "main"]);
    towermd_core::init::init(
        temp.path(),
        &towermd_core::init::InitOptions {
            docs_dir: Some("docs/tower".into()),
        },
    )
    .unwrap();
    // 对象 ID 区分大小写；文件名在 macOS 默认文件系统上也必须互不覆盖。
    for (file, id) in [("module-upper", "A"), ("module-lower", "a")] {
        std::fs::write(
            temp.path().join(format!("docs/tower/{file}.md")),
            format!("---\ntype: Module\nid: {id}\npaths: []\n---\n"),
        )
        .unwrap();
    }
    for (id, fields) in decisions {
        let text = format!("---\ntype: Decision\nid: {id}\n{fields}\n---\n");
        towermd_core::okf::Document::parse(&text).unwrap();
        std::fs::write(temp.path().join(format!("docs/tower/{id}.md")), text).unwrap();
    }
    git(temp.path(), &["add", "."]);
    git(
        temp.path(),
        &["commit", "-qm", "authoritative decision fixtures"],
    );
    let catalog = towermd_core::catalog::Catalog::load(temp.path()).unwrap();
    for id in ["A", "a"] {
        assert_eq!(catalog.objects[id].doc_type(), Some("Module"));
    }
    assert_ne!(catalog.object_paths["A"], catalog.object_paths["a"]);
    (temp, catalog)
}
#[test]
fn authoritative_decision_answer_ignores_worktree_and_binds_revision() {
    let (temp, catalog) =
        decision_catalog(&[("D-1", "context: ＡＢＣ？\ndecision: 阻塞\naffects: [A]")]);
    let answer = decision_answer(&catalog, "A", "abc!", 0).unwrap().unwrap();
    assert_eq!(answer.actor, "process:towermd-dedupe");
    assert_eq!(answer.decision.as_deref(), Some("D-1"));
    assert_eq!(
        answer.decision_revision.as_deref(),
        Some(catalog.revision.as_str())
    );
    std::fs::write(temp.path().join("docs/tower/D-1.md"), "not the authority").unwrap();
    assert_eq!(
        decision_answer(&catalog, "A", "abc!", 0).unwrap(),
        Some(answer)
    );
    assert!(decision_answer(&catalog, "a", "abc!", 0).unwrap().is_none());
    assert!(decision_answer(&catalog, "A", "ab c!", 0)
        .unwrap()
        .is_none());
}
#[test]
fn deprecated_stale_superseded_malformed_and_conflicting_decisions_never_auto_pick() {
    for fields in [
        "status: deprecated",
        "stale_after: 1970-01-01",
        "stale_after: not-a-date",
        "status: future_status",
        "affects: malformed",
    ] {
        let affects = if fields.starts_with("affects:") {
            ""
        } else {
            "affects: [A]\n"
        };
        let (_, catalog) = decision_catalog(&[(
            "D-1",
            &format!("context: question?\ndecision: answer\n{affects}{fields}"),
        )]);
        assert!(
            decision_answer(&catalog, "A", "question", 0)
                .unwrap()
                .is_none(),
            "{fields}"
        );
    }
    let (_, catalog) = decision_catalog(&[
        ("D-old", "context: question?\ndecision: old\naffects: [A]"),
        (
            "D-new",
            "context: another question\ndecision: new\naffects: [A]\nsupersedes: [D-old]",
        ),
    ]);
    assert!(decision_answer(&catalog, "A", "question", 0)
        .unwrap()
        .is_none());
    let (_, catalog) = decision_catalog(&[
        ("D-1", "context: question?\ndecision: one\naffects: [A]"),
        ("D-2", "context: question!\ndecision: two\naffects: [A]"),
    ]);
    assert!(decision_answer(&catalog, "A", "question", 0).is_err());
}

#[test]
fn history_requests_and_unknown_kinds_never_replace_effective_hold() {
    let mut f = Fixture::new();
    let (open, _) = f.open("history only", 0);
    let mut state = HumanState::default();
    state.apply(&open).unwrap();
    let mut runtime = open.clone();
    runtime.kind = "hold.wait_started".into();
    runtime.seq += 1;
    runtime.data["hold"]["waits"] = json!([{"id":uuid::Uuid::now_v7().to_string(),"recipient_run":"R","started_ms":10,"deadline_ms":90010,"finished_ms":null}]);
    runtime.data["hold"]["attention"] = json!(true);
    state.apply(&runtime).unwrap();
    let before = state.projection().0.clone();
    let mut requested = open.clone();
    requested.seq += 2;
    requested.kind = "hold.requested".into();
    state.apply(&requested).unwrap();
    assert_eq!(state.projection().0, &before);
    let mut unknown = open.clone();
    unknown.kind = "hold.future_event".into();
    unknown.seq += 3;
    unknown.data["hold"]["body"]["plan_content"] = json!("tampered");
    state.apply(&unknown).unwrap();
    assert_eq!(state.projection().0, &before);
}
#[test]
fn expired_inbox_keeps_archived_reply_without_actionable_resume() {
    let mut f = Fixture::new();
    let (open, _) = f.open("archived answer", 0);
    let answer = f.answer(&open, ReviewVerdict::Approved);
    let mut state = HumanState::default();
    state.apply(&open).unwrap();
    state.apply(&answer).unwrap();
    let recipients = state.holds[&open.obj].recipients.clone();
    let live = state.inbox(&recipients, &f.fences);
    assert_eq!(live.len(), 1);
    assert!(live[0].actionable);
    let mut stale = f.fences.clone();
    stale[1].epoch += 1;
    let archived = state.inbox(&recipients, &stale);
    assert_eq!(archived.len(), 1);
    assert!(!archived[0].actionable);
    assert_eq!(live[0].id, archived[0].id);
}

#[test]
fn shared_commands_and_hold_bodies_roundtrip_without_losing_extensions() {
    let commands = vec![
        HumanCommand::AskOpen {
            question: "q".into(),
            options: vec!["a".into()],
            recommended: Some(0),
            context: "c".into(),
        },
        HumanCommand::AskWait { hold: "H".into() },
        HumanCommand::Relay {
            hold: "H".into(),
            text: "text".into(),
            self_reported: Some(SelfReported {
                harness: "codex".into(),
                version: "fixture".into(),
            }),
        },
        HumanCommand::ReviewRequest {
            gate: ReviewGate::G1,
            content: Some("plan".into()),
            source_commit: None,
            source_path: Some("plan.md".into()),
        },
        HumanCommand::ReviewWait { hold: "H".into() },
        HumanCommand::PauseAll,
        HumanCommand::ResumeAll,
    ];
    for command in commands {
        let value = serde_json::to_value(&command).unwrap();
        let decoded: HumanCommand = serde_json::from_value(value).unwrap();
        assert_eq!(command, decoded);
    }
    for body in [
        HoldBody::Prompt {
            text: "prompt".into(),
            scope: Scope::Project,
        },
        HoldBody::Prompt {
            text: "prompt".into(),
            scope: Scope::Module { module: "A".into() },
        },
        HoldBody::Prompt {
            text: "prompt".into(),
            scope: Scope::Task { task: "T".into() },
        },
        HoldBody::Pause {
            revision: "a".repeat(40),
            active_runs: vec!["R".into()],
            resumed: false,
        },
        HoldBody::Resume {
            pause_revision: "a".repeat(40),
            active_runs: vec!["R".into()],
            confirmed: false,
        },
    ] {
        let mut hold = ask();
        hold.body = body;
        hold.extra
            .insert("future_hold".into(), json!({"untouched":[1,null]}));
        hold.reply = Some(Reply {
            id: "reply".into(),
            text: "answer".into(),
            actor: "human:fixture".into(),
            relayed_from: None,
            self_reported: None,
            decision: None,
            decision_revision: None,
            at_ms: 20,
            extra: BTreeMap::from([("future_reply".into(), json!(true))]),
        });
        let value = serde_json::to_value(&hold).unwrap();
        let decoded: Hold = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, hold);
    }
    let result = AskResult {
        hold: "H".into(),
        state: AskState::Expired,
        reply: None,
    };
    let decoded: AskResult =
        serde_json::from_value(serde_json::to_value(&result).unwrap()).unwrap();
    assert_eq!(decoded, result);
}

#[test]
fn exact_legacy_git_diagnostic_remains_opaque_but_typed_errors_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(temp.path()).unwrap();
    let commit = "a".repeat(40);
    let event = recorded(hub.append(NewEvent {
        eid: towermd_core::events::derived_eid(&commit, "hold.opened").unwrap(),
        host: "git".into(), actor: "process:towermd-daemon".into(),
        kind: "hold.opened".into(), layer: "L3".into(), obj: "main".into(),
        data: json!({"commit":commit,"reason":"unproved Git trailer","raw_lines":["Towermd-Task: T"]}),
        guards: vec![],
    }, 10).unwrap());
    let original = serde_json::to_value(&event).unwrap();
    let mut state = HumanState::default();
    let before = (state.projection().0.clone(), state.projection().1.clone());
    state.apply(&event).unwrap();
    assert!(state.holds.is_empty());
    assert_eq!(state.projection(), (&before.0, &before.1));
    assert_eq!(serde_json::to_value(&event).unwrap(), original);
    assert!(state.current_plan_subject("R").is_err());

    for field in [
        "layer",
        "actor",
        "host",
        "obj",
        "guards",
        "epoch",
        "eid",
        "commit",
        "reason",
        "raw_lines",
        "raw_line_item",
        "extra",
        "typed_hold",
    ] {
        let mut changed = event.clone();
        match field {
            "layer" => changed.layer = "L5".into(),
            "actor" => changed.actor = "human:fixture".into(),
            "host" => changed.host = "other".into(),
            "obj" => changed.obj = "H-other".into(),
            "guards" => {
                changed.guards = vec![Fence {
                    name: "module:A".into(),
                    holder: "R".into(),
                    epoch: 7,
                }]
            }
            "epoch" => changed.epoch = Some(7),
            "eid" => changed.eid = uuid::Uuid::now_v7().to_string(),
            "commit" => changed.data["commit"] = json!("not-a-full-Git-object-id"),
            "reason" => changed.data["reason"] = json!(false),
            "raw_lines" => changed.data["raw_lines"] = json!("not-an-array"),
            "raw_line_item" => changed.data["raw_lines"] = json!([7]),
            "extra" => changed.data["future_shape"] = json!(true),
            _ => changed.data["hold"] = json!({"type":"Hold","id":"H-disguised"}),
        }
        assert!(
            state.apply(&changed).is_err(),
            "不能将 {field} 伪装成既有诊断而吞掉 typed 错误"
        );
        assert_eq!(state.projection(), (&before.0, &before.1));
        assert!(state.current_plan_subject("R").is_err());
    }
    let mut rebuilt = event.clone();
    rebuilt.kind = "hub.rebuilt".into();
    rebuilt.seq += 1;
    state.apply(&rebuilt).unwrap();
    assert_eq!(state.projection().1.invalidated_through, rebuilt.seq);
    assert!(state.holds.is_empty());
}

// Append to tests/human_intervention.rs after its existing actual-Hub Fixture.
// Contract fixture writes an intentionally legacy None source through the real
// snapshot writer. It does not invoke a live Human producer or App action.
#[path = "support/mod.rs"]
mod session_snapshot_support;

fn session_snapshot_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    let previous = fixture.hub.subscribe(0, 3).unwrap().head.leases["main"].fence();
    fixture
        .hub
        .lease_release(
            towermd_core::hub_protocol::ReleaseLease {
                host: "host".into(),
                fences: vec![previous],
            },
            3,
        )
        .unwrap();
    let main = fixture
        .hub
        .lease_acquire(
            AcquireLease {
                host: "host".into(),
                holder: "snapshot:host".into(),
                names: vec!["main".into()],
                ttl_ms: 1_000_000,
            },
            4,
        )
        .unwrap();
    fixture.main_epoch = main.leases[0].epoch;
    fixture
}

fn persist_legacy_human_source(
    fixture: &mut Fixture,
    catalog: &towermd_core::catalog::Catalog,
) -> (Snapshot, String) {
    let events = fixture.events();
    let through = fixture.hub.contiguous_seq();
    let mut legacy = Snapshot::from_events(
        catalog.config.project_id(),
        &events,
        through,
        fixture.main_epoch,
    )
    .unwrap();
    // This is the original legacy interpretation of the same actual archive:
    // no Human metadata and no Hold projection. No raw Event is rewritten.
    legacy.projection.human = None;
    legacy.projection.objects.remove("Hold");
    legacy
        .validate_archive(catalog.config.project_id(), &events)
        .unwrap();
    let mut archives = BTreeMap::<String, String>::new();
    for event in events.iter().filter(|event| !event.is_pure_heartbeat()) {
        let month = chrono::DateTime::parse_from_rfc3339(&event.t)
            .unwrap()
            .format("%Y-%m")
            .to_string();
        let text = archives.entry(month).or_default();
        text.push_str(&serde_json::to_string(event).unwrap());
        text.push('\n');
    }
    let request = towermd_core::snapshot_git::WriteRequest {
        base: catalog.revision.clone(),
        docs_dir: catalog.config.docs_dir().into(),
        state_json: serde_json::to_string_pretty(&legacy).unwrap(),
        archives,
        through_seq: through,
        writer_epoch: fixture.main_epoch,
    };
    let write = towermd_core::snapshot_git::write_snapshot(&catalog.root, &request, || {
        let leases = fixture
            .hub
            .subscribe(through, 30)
            .map_err(|error| error.to_string())?
            .head
            .leases;
        if leases.get("main").is_some_and(|lease| {
            lease.epoch == fixture.main_epoch
                && lease.holder == "snapshot:host"
                && lease.expires_at > 30
        }) {
            Ok(())
        } else {
            Err("actual fixture main fence changed".into())
        }
    })
    .unwrap();
    let towermd_core::snapshot_git::WriteOutcome::Written { commit } = write else {
        panic!("legacy fixture must actually persist: {write:?}")
    };
    HubProtocol::snapshot_ack(
        &mut fixture.hub,
        towermd_core::hub::SnapshotAck {
            commit: commit.clone(),
            seq: through,
            epoch: fixture.main_epoch,
        },
        serde_json::to_value(&legacy).unwrap(),
    )
    .unwrap();
    (legacy, commit)
}

#[test]
fn legacy_source_and_ack_remain_exact_while_session_human_checkpoint_upgrades() {
    use towermd_core::{catalog::Catalog, project_session::ProjectSession};
    let mut f = session_snapshot_fixture();
    let catalog = session_snapshot_support::catalog(f.temp.path());
    let (opening, binding) = f.open("legacy actual approved plan", 999);
    let answer = f.answer(&opening, ReviewVerdict::Approved);
    let (legacy, commit) = persist_legacy_human_source(&mut f, &catalog);
    let Fixture { temp, hub, .. } = f;
    let current = Catalog::load(temp.path()).unwrap();
    let mut outbox = session_snapshot_support::outbox(&current);
    let inputs = outbox.read_inputs().unwrap();
    let session =
        std::sync::Mutex::new(ProjectSession::new(Box::new(hub), &current, &inputs, 31).unwrap());
    {
        let live = session.lock().unwrap();
        assert_eq!(live.snapshot_ack().unwrap().commit, commit);
        let human = live.human().unwrap();
        assert_eq!(
            human.require_human_review(&binding).unwrap().answer_eid,
            answer.eid
        );
        assert_eq!(human.holds[&opening.obj].opened_seq, opening.seq);
        assert_eq!(human.projection().1.openings[&opening.obj], opening);
        assert_eq!(
            human.projection().0[&opening.obj]["future_hold"],
            json!({"keep":[1,null]})
        );
        assert_eq!(
            human.projection().1.openings[&opening.obj].data["hold"]["opened_seq"],
            999
        );
    }
    let saved = towermd_core::snapshot_service::read_snapshot(&current)
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.snapshot, legacy,
        "runtime hydration must not reinterpret the source"
    );
    assert!(saved.snapshot.projection.human.is_none());
    // An actual new outer record advances Q before the normal writer may save
    // the upgraded checkpoint. Equal-Q None/Some sources remain conflicting.
    session
        .lock()
        .unwrap()
        .protocol()
        .unwrap()
        .append(
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "host".into(),
                actor: "process:fixture".into(),
                kind: "future.audit".into(),
                layer: "L5".into(),
                obj: "audit".into(),
                data: json!({}),
                guards: vec![],
            },
            32,
        )
        .unwrap();
    ProjectSession::sync_shared(&session, &current, &outbox.read_inputs().unwrap(), 33).unwrap();
    let result = towermd_core::snapshot_service::SnapshotWriter::default()
        .write(&session, &current, "host", true, || Ok(34), &mut outbox)
        .unwrap();
    assert_eq!(result.status, "written");
    let latest = Catalog::load(temp.path()).unwrap();
    let upgraded = towermd_core::snapshot_service::read_snapshot(&latest)
        .unwrap()
        .unwrap();
    assert!(upgraded.snapshot.through_seq > legacy.through_seq);
    assert!(upgraded.snapshot.projection.human.is_some());
    let source = serde_json::to_string(&upgraded.snapshot).unwrap();
    drop(session);
    // Reopen the actual Hub checkpoint; its ack and source now both use Some.
    // Fixture Hub was opened at temp root, independently of daemon target paths.
    let reopened =
        ProjectSession::open(temp.path(), &latest, &outbox.read_inputs().unwrap(), 35).unwrap();
    assert_eq!(
        reopened
            .human()
            .unwrap()
            .require_human_review(&binding)
            .unwrap()
            .answer_eid,
        answer.eid
    );
    assert_eq!(
        serde_json::to_string(
            &towermd_core::snapshot_service::read_snapshot(&latest)
                .unwrap()
                .unwrap()
                .snapshot
        )
        .unwrap(),
        source
    );
}

#[test]
fn two_actual_rebuilds_keep_raw_human_history_and_invalidate_old_approval() {
    use towermd_core::{catalog::Catalog, project_session::ProjectSession};
    let mut f = session_snapshot_fixture();
    let catalog = session_snapshot_support::catalog(f.temp.path());
    let (opening, binding) = f.open("approved before actual hub loss", 0);
    let answer = f.answer(&opening, ReviewVerdict::Approved);
    let (legacy, _) = persist_legacy_human_source(&mut f, &catalog);
    let Fixture { temp, hub, .. } = f;
    drop(hub);
    // Exercise the existing real producer of opaque L3 recovery diagnostics.
    session_snapshot_support::git(temp.path(), &["switch", "main"]);
    session_snapshot_support::git(
        temp.path(),
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "unproved legacy trailer\n\nTowermd-Task: T-unknown\nTowermd-Run: R-unknown",
        ],
    );
    session_snapshot_support::git(temp.path(), &["switch", "t/T"]);
    let mut previous_barrier = legacy.through_seq;
    for round in 0..2 {
        let current = Catalog::load(temp.path()).unwrap();
        let inputs = session_snapshot_support::inputs(&current);
        let requested = 100 + round * 100;
        let request = towermd_core::recovery::prepare(
            &current,
            &inputs,
            requested,
            10_000,
            &uuid::Uuid::now_v7().to_string(),
        )
        .unwrap();
        let (_, rebuilt) = towermd_core::snapshot_service::SnapshotWriter::default()
            .confirm_rebuild(
                None,
                &current,
                &request,
                "human:fixture",
                10_000,
                &inputs,
                "host",
                || Ok(requested + 1),
            )
            .unwrap();
        let live = rebuilt.lock().unwrap();
        let human = live.human().unwrap();
        assert!(human.require_human_review(&binding).is_err());
        assert_eq!(human.projection().1.openings[&opening.obj], opening);
        assert_eq!(human.projection().1.reviewed[&opening.obj], answer);
        assert_eq!(human.holds[&opening.obj].opened_seq, opening.seq);
        assert!(!human.holds.contains_key("main"));
        let barrier = human.projection().1.invalidated_through;
        assert!(barrier > previous_barrier);
        assert!(barrier > request.seq_boundary);
        assert!(barrier <= live.applied_seq());
        previous_barrier = barrier;
        drop(live);
        let latest = Catalog::load(temp.path()).unwrap();
        let saved = towermd_core::snapshot_service::read_snapshot(&latest)
            .unwrap()
            .unwrap();
        let archive: Vec<Event> = saved
            .archives
            .values()
            .flat_map(|text| text.lines().map(|line| serde_json::from_str(line).unwrap()))
            .collect();
        saved
            .snapshot
            .validate_archive(latest.config.project_id(), &archive)
            .unwrap();
        assert!(archive.iter().any(|event| event.kind == "hold.opened"
            && event.actor == "process:towermd-daemon"
            && event.layer == "L3"
            && event.host == "git"
            && event.obj == "main"
            && event.data.get("hold").is_none()));
        let checkpoint = serde_json::to_value(&saved.snapshot).unwrap();
        drop(rebuilt);
        let reopened = ProjectSession::open(
            &latest.common_dir.join("towermd"),
            &latest,
            &session_snapshot_support::inputs(&latest),
            requested + 2,
        )
        .unwrap();
        assert!(reopened
            .human()
            .unwrap()
            .require_human_review(&binding)
            .is_err());
        assert_eq!(
            reopened.human().unwrap().projection().1.invalidated_through,
            barrier
        );
        assert_eq!(
            serde_json::to_value(
                &towermd_core::snapshot_service::read_snapshot(&latest)
                    .unwrap()
                    .unwrap()
                    .snapshot
            )
            .unwrap(),
            checkpoint
        );
        drop(reopened);
        // A second real journal loss must rely on Git/archive, not old outbox
        // review receipts or a process-local HumanState cache.
        std::fs::remove_dir_all(latest.common_dir.join("towermd")).unwrap();
    }
}

#[test]
fn old_none_ack_and_new_some_selected_source_are_proved_in_their_own_modes() {
    use towermd_core::{catalog::Catalog, project_session::ProjectSession};
    let mut f = session_snapshot_fixture();
    let catalog = session_snapshot_support::catalog(f.temp.path());
    let (opening, binding) = f.open("old ack new actual source", 999);
    f.answer(&opening, ReviewVerdict::Approved);
    let (legacy, old_commit) = persist_legacy_human_source(&mut f, &catalog);
    let current = Catalog::load(f.temp.path()).unwrap();
    let source = towermd_core::snapshot_service::read_snapshot(&current)
        .unwrap()
        .unwrap();
    let old_blob_path = format!(
        "{old_commit}:{}/state/state.json",
        current.config.docs_dir()
    );
    let old_bytes = session_snapshot_support::git(f.temp.path(), &["show", &old_blob_path]);
    let audit = recorded(
        f.hub
            .append(
                NewEvent {
                    eid: uuid::Uuid::now_v7().to_string(),
                    host: "host".into(),
                    actor: "process:fixture".into(),
                    kind: "future.audit".into(),
                    layer: "L5".into(),
                    obj: "audit".into(),
                    data: json!({}),
                    guards: vec![],
                },
                32,
            )
            .unwrap(),
    );
    let mut events: Vec<Event> = source
        .archives
        .values()
        .flat_map(|text| text.lines().map(|line| serde_json::from_str(line).unwrap()))
        .collect();
    events.push(audit.clone());
    events.sort_by_key(|event| event.seq);
    let next = Snapshot::from_events(
        current.config.project_id(),
        &events,
        audit.seq,
        legacy.writer_epoch,
    )
    .unwrap();
    next.validate_archive(current.config.project_id(), &events)
        .unwrap();
    assert!(next.projection.human.is_some());
    assert_ne!(next.projection.human, legacy.projection.human);
    let mut archives = source.archives;
    let month = chrono::DateTime::parse_from_rfc3339(&audit.t)
        .unwrap()
        .format("%Y-%m")
        .to_string();
    let text = archives.entry(month).or_default();
    text.push_str(&serde_json::to_string(&audit).unwrap());
    text.push('\n');
    let request = towermd_core::snapshot_git::WriteRequest {
        base: current.revision.clone(),
        docs_dir: current.config.docs_dir().into(),
        state_json: serde_json::to_string_pretty(&next).unwrap(),
        archives,
        through_seq: next.through_seq,
        writer_epoch: next.writer_epoch,
    };
    let result = towermd_core::snapshot_git::write_snapshot(&current.root, &request, || {
        let head = f
            .hub
            .subscribe(audit.seq, 33)
            .map_err(|error| error.to_string())?
            .head;
        if head.leases.get("main").is_some_and(|lease| {
            lease.epoch == legacy.writer_epoch
                && lease.holder == "snapshot:host"
                && lease.expires_at > 33
        }) {
            Ok(())
        } else {
            Err("actual fixture writer changed".into())
        }
    })
    .unwrap();
    let towermd_core::snapshot_git::WriteOutcome::Written { commit: new_commit } = result else {
        panic!("new actual source must be written: {result:?}")
    };
    // Deliberately leave the actual Hub ack at the older None checkpoint.
    assert_eq!(f.hub.snapshot_ack().unwrap().commit, old_commit);
    let Fixture { temp, hub, .. } = f;
    let latest = Catalog::load(temp.path()).unwrap();
    let selected = towermd_core::snapshot_service::read_snapshot(&latest)
        .unwrap()
        .unwrap();
    assert_eq!(selected.commit, new_commit);
    assert_eq!(selected.snapshot, next);
    let live = ProjectSession::new(
        Box::new(hub),
        &latest,
        &session_snapshot_support::inputs(&latest),
        34,
    )
    .unwrap();
    assert_eq!(live.snapshot_ack().unwrap().commit, old_commit);
    assert_eq!(live.applied_seq(), audit.seq);
    live.human()
        .unwrap()
        .require_human_review(&binding)
        .unwrap();
    assert_eq!(
        live.human().unwrap().projection().1.openings[&opening.obj],
        opening
    );
    assert_eq!(
        session_snapshot_support::git(temp.path(), &["show", &old_blob_path]),
        old_bytes
    );
    assert!(
        serde_json::from_str::<Value>(&old_bytes).unwrap()["projection"]
            .get("human")
            .is_none()
    );
}

#[test]
fn legacy_none_and_human_some_archive_modes_remain_strict_and_detect_tampering() {
    let mut f = Fixture::new();
    let (opening, binding) = f.open("mode proof actual plan", 999);
    f.answer(&opening, ReviewVerdict::Approved);
    let events = f.events();
    let some = Snapshot::from_events("project", &events, events.last().unwrap().seq, f.main_epoch)
        .unwrap();
    some.validate_archive("project", &events).unwrap();
    let mut legacy = some.clone();
    legacy.projection.human = None;
    legacy.projection.objects.remove("Hold");
    legacy.validate_archive("project", &events).unwrap();
    let legacy_bytes = serde_json::to_vec(&legacy).unwrap();
    let legacy_json: Value = serde_json::from_slice(&legacy_bytes).unwrap();
    assert!(legacy_json["projection"].get("human").is_none());
    let decoded: Snapshot = serde_json::from_slice(&legacy_bytes).unwrap();
    assert!(decoded.projection.human.is_none());
    assert_ne!(
        some, decoded,
        "same Q/epoch do not make None and Some equivalent"
    );
    let extended = Snapshot::extend(
        Some(&decoded),
        &[],
        decoded.through_seq,
        decoded.writer_epoch,
    )
    .unwrap();
    assert_eq!(
        extended, decoded,
        "tail-only extend must retain the legacy mode"
    );
    let mut state = HumanState::default();
    state.restore(&decoded.projection).unwrap();
    assert!(state.require_human_review(&binding).is_err());
    for field in [
        "schema",
        "raw_opening",
        "effective_seq",
        "missing_metadata",
        "injected_metadata",
    ] {
        let mut changed = some.clone();
        match field {
            "schema" => changed.projection.human.as_mut().unwrap().schema += 1,
            "raw_opening" => {
                changed
                    .projection
                    .human
                    .as_mut()
                    .unwrap()
                    .openings
                    .get_mut(&opening.obj)
                    .unwrap()
                    .data["hold"]["future_hold"] = json!("changed")
            }
            "effective_seq" => {
                changed
                    .projection
                    .objects
                    .get_mut("Hold")
                    .unwrap()
                    .get_mut(&opening.obj)
                    .unwrap()["opened_seq"] = json!(999)
            }
            "missing_metadata" => changed.projection.human = None,
            _ => {
                changed = decoded.clone();
                changed.projection.human = Some(HumanProjection::default());
            }
        }
        assert!(
            changed.validate_archive("project", &events).is_err(),
            "{field}"
        );
    }
}
