use super::*;
fn git(root: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=codex/gpt-6",
            "-c",
            "user.email=codex@towermd.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .into()
}
fn catalog() -> (tempfile::TempDir, Catalog) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-qb", "main"]);
    crate::init::init(
        root.path(),
        &crate::init::InitOptions {
            docs_dir: Some("docs/tower".into()),
        },
    )
    .unwrap();
    git(root.path(), &["add", "."]);
    git(root.path(), &["commit", "-qm", "contract history fixture"]);
    let c = Catalog::load(root.path()).unwrap();
    (root, c)
}
// Contract history fixture: real local hub receipts over synthetic producer data.
// No App click or verifier execution is claimed by this fixture.
struct MergeFixture {
    _root: tempfile::TempDir,
    storage: tempfile::TempDir,
    _hubdir: tempfile::TempDir,
    catalog: Catalog,
    outbox: crate::recovery_outbox::RecoveryOutbox,
    hub: crate::hub::Hub,
    input: MergeProofInput,
    claim: Event,
    started: Event,
    completed: Event,
    integration_gate: Event,
}
impl MergeFixture {
    fn new(high: bool) -> Self {
        Self::with_shape(high, "single")
    }
    fn with_shape(high: bool, shape: &str) -> Self {
        use crate::hub_protocol::*;
        let (root, _) = catalog();
        std::fs::write(
            root.path().join("docs/tower/M.md"),
            format!(
                "---\ntype: Module\nid: M\nlayer: L1\npaths: [code.txt, other.txt]\nrisk: {}\n---\n",
                if high { "high" } else { "low" }
            ),
        )
        .unwrap();
        if matches!(shape, "consumer" | "consumer-passed-file") {
            std::fs::write(
                root.path().join("docs/tower/AC.md"),
                "---\ntype: Acceptance\nid: AC\nmodule: M\ncriteria: [works]\n---\n",
            )
            .unwrap();
        }
        if shape == "consumer-passed-file" {
            std::fs::write(
                root.path().join("docs/tower/E.md"),
                "---\ntype: Evidence\nid: E\nkind: test\ntask: T\nrun: R\nmodule: M\nresult: passed\ngenerated:\n  by: process:towermd-verifier\n---\n\nContract isolated passed file; no original verifier receipt.\n",
            ).unwrap();
        }
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-qm", "original Module"]);
        let base = git(root.path(), &["rev-parse", "HEAD"]);
        git(root.path(), &["switch", "-qc", "contract-task"]);
        if shape == "multi" {
            std::fs::write(root.path().join("code.txt"), "intermediate Task\n").unwrap();
            git(root.path(), &["add", "code.txt"]);
            git(root.path(), &["commit", "-qm", "intermediate Task commit"]);
        }
        std::fs::write(root.path().join("code.txt"), "task\n").unwrap();
        git(root.path(), &["add", "code.txt"]);
        let trailers = TaskTrailers {
            task: "T".into(),
            run: "R".into(),
            module: "M".into(),
            evidence: "E".into(),
            epoch: 1,
        };
        git(
            root.path(),
            &[
                "commit",
                "-qm",
                &if shape == "multi_parent" {
                    "pre-final Task tip".to_owned()
                } else {
                    format!(
                        "actual task\n\n{}",
                        render_task_trailers(&trailers).unwrap()
                    )
                },
            ],
        );
        if shape == "multi_parent" {
            let tip = git(root.path(), &["rev-parse", "HEAD"]);
            let tree = git(root.path(), &["rev-parse", "HEAD^{tree}"]);
            let base_tree = git(root.path(), &["rev-parse", &format!("{base}^{{tree}}")]);
            let side = git(
                root.path(),
                &[
                    "commit-tree",
                    &base_tree,
                    "-p",
                    &base,
                    "-m",
                    "parallel Task history",
                ],
            );
            let merge = git(
                root.path(),
                &[
                    "commit-tree",
                    &tree,
                    "-p",
                    &tip,
                    "-p",
                    &side,
                    "-m",
                    &format!(
                        "Task own merge\n\n{}",
                        render_task_trailers(&trailers).unwrap()
                    ),
                ],
            );
            git(
                root.path(),
                &["update-ref", "refs/heads/contract-task", &merge, &tip],
            );
        }
        let catalog = Catalog::load(root.path()).unwrap();
        let commit = git(root.path(), &["rev-parse", "HEAD"]);
        let tree = git(root.path(), &["rev-parse", "HEAD^{tree}"]);
        let hd = tempfile::tempdir().unwrap();
        let mut hub = crate::hub::Hub::open(hd.path()).unwrap();
        let fences = hub
            .lease_acquire(
                AcquireLease {
                    host: "h".into(),
                    names: vec!["module:M".into()],
                    holder: "R".into(),
                    ttl_ms: 100000,
                },
                100,
            )
            .unwrap()
            .leases
            .iter()
            .map(|l| l.fence())
            .collect::<Vec<_>>();
        hub.lease_acquire(
            AcquireLease {
                host: "h".into(),
                names: vec!["main".into()],
                holder: "original-writer".into(),
                ttl_ms: 100000,
            },
            100,
        )
        .unwrap();
        let task = json!({"type":"Task","id":"T","title":"fixture","module":"M","touches":[],"allowed_paths":["code.txt"],"depends_on":[],"acceptance":[],"decisions":[],"state":"running","x-original":{"keep":true}});
        let run = json!({"type":"Run","id":"R","task":"T","host":"h","worktree":"original-location","branch":"task","state":"running","leases":fences,"cost_usd":null,"task_card":{"original":true},"x-original":4});
        let storage = tempfile::tempdir().unwrap();
        let mut outbox = crate::recovery_outbox::RecoveryOutbox::open(
            storage.path(),
            crate::recovery_outbox::RecoveryContext::from_catalog(&catalog).unwrap(),
        )
        .unwrap();
        let claim = append_fixture(
            &mut hub,
            &mut outbox,
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "h".into(),
                actor: "process:towermd-cli".into(),
                kind: "task.claimed".into(),
                layer: "L5".into(),
                obj: "T".into(),
                data: json!({"task":task,"run":run}),
                guards: fences.clone(),
            },
        );
        let commands = vec![VerificationCommand {
            cwd: ".".into(),
            argv: vec!["true".into()],
        }];
        let mut frozen = FrozenFinish {
            evidence_id: "E".into(),
            project_id: catalog.config.project_id().into(),
            host: "h".into(),
            root: "/original/root".into(),
            common_dir: "/original/common".into(),
            task_id: "T".into(),
            run_id: "R".into(),
            module_id: "M".into(),
            commit: commit.clone(),
            tree: tree.clone(),
            base: base.clone(),
            authority: base,
            baseline: "towermd/B0".into(),
            plan_hash: None,
            commands_hash: commands_hash(&commands).unwrap(),
            commands: commands.clone(),
            fences: fences.clone(),
            entered_verifying_at: 101,
            merge_deadline: 10000,
        };
        let mut verifying = task;
        verifying["state"] = json!("verifying");
        if high {
            frozen.plan_hash = Some(contract::sha256(b"contract plan"));
        }
        let g1 = high
            .then(|| review_fixture(&mut hub, &mut outbox, &frozen, ReviewGate::G1, None, None));
        let started = append_fixture(
            &mut hub,
            &mut outbox,
            NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "h".into(),
                actor: "process:towermd-verifier".into(),
                kind: "finish.started".into(),
                layer: "L4".into(),
                obj: "T".into(),
                data: json!({"task":verifying,"run":run,"input":frozen,"review_context":g1}),
                guards: frozen.fences.clone(),
            },
        );
        let report = VerificationReport {
            commit: commit.clone(),
            tree,
            executions: vec![CommandExecution {
                command: commands[0].clone(),
                result: "passed".into(),
                exit_code: Some(0),
                timed_out: false,
                stdout_log: "original:stdout".into(),
                stderr_log: "original:stderr".into(),
                stdout_sha256: contract::sha256(b""),
                stderr_sha256: contract::sha256(b""),
                diagnostic: None,
            }],
        };
        let eid = uuid::Uuid::now_v7().to_string();
        let evidence = FinishEvidence {
            id: "E".into(),
            kind: "test".into(),
            task: "T".into(),
            run: "R".into(),
            module: "M".into(),
            commit: commit.clone(),
            tree: frozen.tree.clone(),
            base: frozen.base.clone(),
            authority: frozen.authority.clone(),
            baseline: frozen.baseline.clone(),
            plan_hash: frozen.plan_hash.clone(),
            commands_hash: frozen.commands_hash.clone(),
            fences: frozen.fences.clone(),
            result: EvidenceResult::Passed,
            generated_by: "process:towermd-verifier".into(),
            log: VerificationLogRef {
                report_eid: eid.clone(),
                report_sha256: report_hash(&report).unwrap(),
            },
            gates: BTreeMap::from([
                ("G0".into(), true),
                ("G2".into(), true),
                ("G3".into(), true),
                ("G5".into(), true),
                ("G4".into(), false),
            ]),
            extra: BTreeMap::from([("type".into(), json!("Evidence"))]),
        };
        let original = NewEvent {
            eid,
            host: "h".into(),
            actor: "process:towermd-verifier".into(),
            kind: "finish.completed".into(),
            layer: "L4".into(),
            obj: "T".into(),
            data: json!({"task":verifying,"run":run,"evidence":evidence,"report":report,"hold":null,"review_context":g1}),
            guards: frozen.fences.clone(),
        };
        let AppendReceipt::Event(completed) =
            crate::recovery_outbox::append_durable(&mut hub, &mut outbox, original, 102).unwrap()
        else {
            panic!()
        };
        if high {
            review_fixture(
                &mut hub,
                &mut outbox,
                &frozen,
                ReviewGate::G4,
                Some(&completed.eid),
                Some(&g1.as_ref().unwrap().opening.eid),
            );
        }
        let integration_gate = terminal_fixture(
            &mut hub,
            &mut outbox,
            &catalog,
            &frozen,
            &completed,
            g1.as_ref(),
        );
        git(
            root.path(),
            &["update-ref", "refs/heads/main", &commit, &frozen.base],
        );
        git(root.path(), &["switch", "-q", "main"]);
        let catalog = Catalog::load(root.path()).unwrap();
        Self {
            _root: root,
            storage,
            _hubdir: hd,
            catalog,
            outbox,
            hub,
            input: MergeProofInput { commit, trailers },
            claim,
            started,
            completed: *completed,
            integration_gate,
        }
    }
}
fn rewrite_receipt(storage: &tempfile::TempDir, event: &Event) {
    for entry in std::fs::read_dir(storage.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let mut lines = Vec::new();
        for line in text.lines() {
            let mut v: Value = serde_json::from_str(line).unwrap();
            if v["original"]["eid"] == event.eid {
                v["original"] =
                    serde_json::to_value(crate::recovery_outbox::original_of(event)).unwrap();
                if v["state"] == "confirmed" {
                    v["actual"] = serde_json::to_value(event).unwrap();
                }
            }
            lines.push(serde_json::to_string(&v).unwrap());
        }
        std::fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
    }
}
fn append_fixture(
    hub: &mut crate::hub::Hub,
    outbox: &mut crate::recovery_outbox::RecoveryOutbox,
    event: NewEvent,
) -> Event {
    let crate::hub_protocol::AppendReceipt::Event(e) =
        crate::recovery_outbox::append_durable(hub, outbox, event, 103).unwrap()
    else {
        panic!()
    };
    *e
}
fn terminal_fixture(
    hub: &mut crate::hub::Hub,
    outbox: &mut crate::recovery_outbox::RecoveryOutbox,
    catalog: &Catalog,
    f: &FrozenFinish,
    completed: &Event,
    g1: Option<&ReviewContext>,
) -> Event {
    let u = uuid::Uuid::now_v7().to_string();
    let evidence = GateEvidence {
        object_type: "Evidence".into(),
        id: format!("E-{u}"),
        kind: "gate".into(),
        gate: crate::task_gates::Gate::G5,
        project_id: f.project_id.clone(),
        task: f.task_id.clone(),
        run: f.run_id.clone(),
        module: f.module_id.clone(),
        commit: Some(f.commit.clone()),
        tree: Some(f.tree.clone()),
        base: Some(f.base.clone()),
        authority: Some(f.authority.clone()),
        baseline: Some(f.baseline.clone()),
        plan_hash: f.plan_hash.clone(),
        plan_opening_eid: g1.map(|c| c.opening.eid.clone()),
        commands_hash: Some(f.commands_hash.clone()),
        fences: f.fences.clone(),
        result: GateResult::Passed,
        review: None,
        diagnostic: None,
        generated: GateGenerated {
            by: "process:towermd-gates".into(),
            at: "1970-01-01T00:00:00.103Z".into(),
        },
        extra: BTreeMap::new(),
    };
    let data = IntegrationGateRecorded {
        evidence,
        completed_eid: completed.eid.clone(),
        parents: commit_parents(catalog, &f.commit).unwrap(),
    };
    append_fixture(
        hub,
        outbox,
        NewEvent {
            eid: request_event_eid(&u, "evidence.recorded").unwrap(),
            host: "h".into(),
            actor: "process:towermd-gates".into(),
            kind: "evidence.recorded".into(),
            layer: "L5".into(),
            obj: data.evidence.id.clone(),
            data: serde_json::to_value(data).unwrap(),
            guards: f.fences.clone(),
        },
    )
}
fn review_fixture(
    hub: &mut crate::hub::Hub,
    outbox: &mut crate::recovery_outbox::RecoveryOutbox,
    f: &FrozenFinish,
    gate: ReviewGate,
    completed: Option<&str>,
    plan_opening: Option<&str>,
) -> ReviewContext {
    use contract::task_review::*;
    let subject = match gate {
        ReviewGate::G1 => ReviewSubject::Plan {
            sha256: f.plan_hash.clone().unwrap(),
        },
        ReviewGate::G4 => ReviewSubject::Tree {
            tree: f.tree.clone(),
        },
    };
    let binding = ReviewBinding {
        run: f.run_id.clone(),
        task: f.task_id.clone(),
        baseline: f.baseline.clone(),
        fences: f.fences.clone(),
        subject,
    };
    let mut hold = ReviewHoldView {
        object_type: "Hold".into(),
        id: format!("H-{}", uuid::Uuid::now_v7()),
        body: ReviewBodyView::Review {
            binding: binding.clone(),
            verdict: None,
            plan_content: if gate == ReviewGate::G1 {
                Some("contract plan".into())
            } else {
                None
            },
            source_commit: None,
            source_path: None,
            comment: None,
        },
        opened_ms: 103,
        opened_seq: 999,
        reply: None,
        extra: BTreeMap::new(),
    };
    let opening = append_fixture(
        hub,
        outbox,
        NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "h".into(),
            actor: "process:towermd-cli".into(),
            kind: "hold.opened".into(),
            layer: "L5".into(),
            obj: hold.id.clone(),
            data: json!({"hold":hold}),
            guards: f.fences.clone(),
        },
    );
    let request = uuid::Uuid::now_v7().to_string();
    let eid = request_event_eid(&request, "hold.reviewed").unwrap();
    let fact = ReviewFact {
        binding: binding.clone(),
        verdict: ReviewVerdict::Approved,
        actor: "human:fixture".into(),
        answer_eid: eid.clone(),
    };
    let ReviewBodyView::Review {
        verdict, comment, ..
    } = &mut hold.body;
    *verdict = Some(ReviewVerdict::Approved);
    *comment = Some("approve contract fixture".into());
    hold.reply = Some(ReviewReplyView {
        id: "reply".into(),
        text: "approve contract fixture".into(),
        actor: fact.actor.clone(),
        at_ms: 103,
        extra: BTreeMap::new(),
    });
    let body = RequestBody {
        project_id: f.project_id.clone(),
        actor: fact.actor.clone(),
        host: "h".into(),
        run: None,
        action: TaskReviewAction::Review {
            hold: hold.id.clone(),
            binding,
            verdict: ReviewVerdict::Approved,
            comment: "approve contract fixture".into(),
        },
    };
    let meta = RequestMeta {
        request_id: request,
        body_hash: contract::canonical_hash(&body).unwrap(),
    };
    let result = TaskReviewResult {
        hold: hold.id.clone(),
        fact: fact.clone(),
        verdict: ReviewVerdict::Approved,
        comment: "approve contract fixture".into(),
    };
    let recorded = TaskReviewRecorded {
        hold: hold.clone(),
        fact,
        comment: "approve contract fixture".into(),
        request_body: body,
        p3_request: meta.clone(),
        p3_receipt: RequestReceipt {
            request_id: meta.request_id,
            body_hash: meta.body_hash,
            result_ref: result,
        },
    };
    let reviewed = append_fixture(
        hub,
        outbox,
        NewEvent {
            eid,
            host: "h".into(),
            actor: "human:fixture".into(),
            kind: "hold.reviewed".into(),
            layer: "L5".into(),
            obj: hold.id,
            data: serde_json::to_value(recorded).unwrap(),
            guards: f.fences.clone(),
        },
    );
    let u = uuid::Uuid::now_v7().to_string();
    let evidence = GateEvidence {
        object_type: "Evidence".into(),
        id: format!("E-{u}"),
        kind: "gate".into(),
        gate: if gate == ReviewGate::G1 {
            crate::task_gates::Gate::G1
        } else {
            crate::task_gates::Gate::G4
        },
        project_id: f.project_id.clone(),
        task: f.task_id.clone(),
        run: f.run_id.clone(),
        module: f.module_id.clone(),
        commit: Some(f.commit.clone()),
        tree: Some(f.tree.clone()),
        base: Some(f.base.clone()),
        authority: Some(f.authority.clone()),
        baseline: Some(f.baseline.clone()),
        plan_hash: f.plan_hash.clone(),
        plan_opening_eid: if gate == ReviewGate::G1 {
            Some(opening.eid.clone())
        } else {
            plan_opening.map(String::from)
        },
        commands_hash: Some(f.commands_hash.clone()),
        fences: f.fences.clone(),
        result: GateResult::Passed,
        review: Some(GateReviewLink {
            opening: opening.clone(),
            reviewed: reviewed.clone(),
            completed_eid: completed.map(String::from),
        }),
        diagnostic: None,
        generated: GateGenerated {
            by: "process:towermd-gates".into(),
            at: "1970-01-01T00:00:00.103Z".into(),
        },
        extra: BTreeMap::new(),
    };
    let gate_evidence = append_fixture(
        hub,
        outbox,
        NewEvent {
            eid: request_event_eid(&u, "evidence.recorded").unwrap(),
            host: "h".into(),
            actor: "process:towermd-gates".into(),
            kind: "evidence.recorded".into(),
            layer: "L5".into(),
            obj: evidence.id.clone(),
            data: json!({"evidence":evidence}),
            guards: f.fences.clone(),
        },
    );
    ReviewContext {
        gate,
        opening,
        reviewed,
        gate_evidence,
    }
}
#[test]
fn high_risk_merge_uses_original_opening_and_late_g4_without_changing_completed() {
    let f = MergeFixture::new(true);
    let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    let event = prove_merge(&f.catalog, &history, &f.input)
        .unwrap()
        .into_event();
    let contexts = event.data["proof"]["review_contexts"].as_array().unwrap();
    assert_eq!(contexts.len(), 2);
    assert!(contexts[1]["gate_evidence"]["seq"].as_u64().unwrap() > f.completed.seq);
    assert_eq!(
        event.data["proof"]["completed"],
        serde_json::to_value(&f.completed).unwrap()
    );
}
#[test]
fn known_later_plan_opening_rejects_old_review_context() {
    let mut f = MergeFixture::new(true);
    let mut opening: Event =
        serde_json::from_value(f.started.data["review_context"]["opening"].clone()).unwrap();
    let content = "different later contract plan";
    opening.data["hold"]["id"] = json!("H-later");
    opening.data["hold"]["body"]["plan_content"] = json!(content);
    opening.data["hold"]["body"]["binding"]["subject"]["sha256"] =
        json!(contract::sha256(content.as_bytes()));
    let mut original = crate::recovery_outbox::original_of(&opening);
    original.eid = uuid::Uuid::now_v7().to_string();
    original.obj = "H-later".into();
    append_fixture(&mut f.hub, &mut f.outbox, original);
    let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    let error = prove_merge(&f.catalog, &history, &f.input).err().unwrap();
    assert!(error.contains("later opening"), "{error}");
}
#[test]
fn raw_pending_and_incomplete_claim_freeze_or_pass_cannot_certify_merge() {
    for kind in ["task.claimed", "finish.started", "finish.completed"] {
        let f = MergeFixture::new(false);
        for entry in std::fs::read_dir(f.storage.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|p| p.to_str()) != Some("jsonl") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let lines = text
                .lines()
                .filter(|line| {
                    let v: Value = serde_json::from_str(line).unwrap();
                    !(v["state"] == "confirmed" && v["original"]["type"] == kind)
                })
                .collect::<Vec<_>>();
            std::fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
        }
        let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
        assert!(
            prove_merge(&f.catalog, &history, &f.input).is_err(),
            "{kind}"
        );
    }
}
#[test]
fn merge_rejects_real_code_outside_original_claimed_allowed_paths() {
    let f = MergeFixture::new(false);
    for mut event in [f.claim.clone(), f.started.clone(), f.completed.clone()] {
        event.data["task"]["allowed_paths"] = json!(["other.txt"]);
        rewrite_receipt(&f.storage, &event);
    }
    let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    assert!(prove_merge(&f.catalog, &history, &f.input).is_err());
}
#[test]
fn wrong_touches_report_tree_actor_or_original_seq_cannot_prove_merge() {
    for change in ["touches", "tree", "actor", "order", "stale"] {
        let f = MergeFixture::new(false);
        let mut event = match change {
            "touches" => f.claim.clone(),
            "order" => f.started.clone(),
            _ => f.completed.clone(),
        };
        match change {
            "touches" => event.data["task"]["touches"] = json!(["undeclared"]),
            "tree" => event.data["evidence"]["tree"] = json!("f".repeat(40)),
            "actor" => event.actor = "process:self-reported".into(),
            "order" => event.seq = f.completed.seq + 1,
            "stale" => event.stale = true,
            _ => unreachable!(),
        }
        rewrite_receipt(&f.storage, &event);
        let inputs = match f.outbox.read_inputs() {
            Ok(inputs) => inputs,
            Err(error) => {
                assert_eq!(change, "order");
                assert!(error.contains("sequence"));
                continue;
            }
        };
        let history = read_recovery_history(&f.catalog, &inputs).unwrap();
        assert!(
            prove_merge(&f.catalog, &history, &f.input).is_err(),
            "{change}"
        );
    }
}
fn persist_recovered_archive(
    catalog: &Catalog,
    source_events: Vec<Event>,
    originals: &[Event],
    high_water: (u64, u64),
    canonical: &NewEvent,
) {
    use crate::hub_protocol::*;
    let source_seq = source_events.last().unwrap().seq;
    let source_epoch = source_events
        .iter()
        .flat_map(|e| e.acquired_leases())
        .find(|l| l.name == "main")
        .unwrap()
        .epoch;
    let mut base = crate::snapshot::Snapshot::from_events(
        catalog.config.project_id(),
        &source_events,
        source_seq,
        source_epoch,
    )
    .unwrap();
    let boundary = (1u64 << 32) + high_water.0.max(source_seq);
    let epoch = (1u64 << 32) + high_water.1.max(source_epoch);
    base.through_seq = boundary;
    base.epoch_high_water = epoch;
    base.recovery_gaps.push(crate::snapshot::SequenceRange {
        start: source_seq + 1,
        end: boundary,
    });
    let newdir = tempfile::tempdir().unwrap();
    let mut next = crate::hub::Hub::open(newdir.path()).unwrap();
    next.rebuild(RebuildSeed {
        final_snapshot: None,
        source_seq,
        source_epoch,
        seq_boundary: boundary,
        epoch_boundary: epoch,
        checkpoint: serde_json::to_value(&base).unwrap(),
        receipts: source_events
            .iter()
            .map(|e| (e.eid.clone(), e.seq))
            .collect(),
        tail: vec![],
    })
    .unwrap();
    next.append(
        NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "new-host".into(),
            actor: "process:towermd-daemon".into(),
            kind: "hub.rebuilt".into(),
            layer: "L3".into(),
            obj: "main".into(),
            data: json!({}),
            guards: vec![],
        },
        104,
    )
    .unwrap();
    for old in originals {
        let AppendReceipt::Event(outer) = next
            .append(crate::recovery_outbox::original_of(old), 104)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(outer.stale, !old.guards.is_empty());
        assert!(outer.seq > boundary);
    }
    next.append(canonical.clone(), 104).unwrap();
    let writer = next
        .lease_acquire(
            AcquireLease {
                host: "new-host".into(),
                names: vec!["main".into()],
                holder: "new-writer".into(),
                ttl_ms: 100000,
            },
            104,
        )
        .unwrap()
        .leases[0]
        .fence();
    let tail = next.subscribe(boundary, 104).unwrap().events;
    let checkpoint =
        crate::snapshot::Snapshot::extend(Some(&base), &tail, next.contiguous_seq(), writer.epoch)
            .unwrap();
    let mut archive = source_events;
    archive.extend(tail);
    checkpoint
        .validate_archive(catalog.config.project_id(), &archive)
        .unwrap();
    let mut archives = BTreeMap::<String, String>::new();
    for event in &archive {
        let month = event.t[..7].to_string();
        archives
            .entry(month)
            .or_default()
            .push_str(&format!("{}\n", serde_json::to_string(event).unwrap()));
    }
    let request = crate::snapshot_git::WriteRequest {
        base: catalog.revision.clone(),
        docs_dir: catalog.config.docs_dir().into(),
        state_json: serde_json::to_string(&checkpoint).unwrap(),
        archives,
        through_seq: checkpoint.through_seq,
        writer_epoch: writer.epoch,
    };
    assert!(matches!(
        crate::snapshot_git::write_snapshot(&catalog.root, &request, || Ok(())).unwrap(),
        crate::snapshot_git::WriteOutcome::Written { .. }
    ));
}
#[test]
fn second_recovery_uses_canonical_originals_without_old_outbox_or_hub() {
    for high in [false, true] {
        let f = MergeFixture::new(high);
        let inputs = f.outbox.read_inputs().unwrap();
        let history = read_recovery_history(&f.catalog, &inputs).unwrap();
        let canonical = prove_merge(&f.catalog, &history, &f.input)
            .unwrap()
            .into_event();
        persist_recovered_archive(
            &f.catalog,
            f.hub.events(0)[..2].to_vec(),
            inputs.confirmed(),
            inputs.high_water(),
            &canonical,
        );
        drop(f.outbox);
        drop(f.hub);
        std::fs::remove_dir_all(f.storage.path()).unwrap();
        let inputs = RecoveryInputs::read(f.storage.path(), &f.catalog).unwrap();
        assert!(inputs.confirmed().is_empty());
        let history = read_recovery_history(&f.catalog, &inputs).unwrap();
        assert_eq!(
            canonical,
            prove_merge(&f.catalog, &history, &f.input)
                .unwrap()
                .into_event()
        );
        assert!(!f.storage.path().exists());
    }
}
fn replace_task_main(f: &mut MergeFixture, mode: &str) -> String {
    let base = f.started.data["input"]["base"].as_str().unwrap();
    let tip = f.input.commit.clone();
    let tree = git(f._root.path(), &["rev-parse", &format!("{tip}^{{tree}}")]);
    let mut parents = match mode {
        "amend" => commit_parents(&f.catalog, &tip).unwrap(),
        "squash" => vec![base.into()],
        _ => vec![base.into(), tip.clone()],
    };
    if mode == "code_revert" {
        let advance = git(
            f._root.path(),
            &[
                "commit-tree",
                &tree,
                "-p",
                base,
                "-m",
                "main code advance after terminal G5",
            ],
        );
        let old_tree = git(f._root.path(), &["rev-parse", &format!("{base}^{{tree}}")]);
        let revert = git(
            f._root.path(),
            &[
                "commit-tree",
                &old_tree,
                "-p",
                &advance,
                "-m",
                "main code reverted before merge",
            ],
        );
        parents[0] = revert;
    }
    let message = if mode == "missing_footer" {
        "main merge without required Task context".to_owned()
    } else {
        format!(
            "actual {mode} main entry\n\n{}",
            render_task_trailers(&f.input.trailers).unwrap()
        )
    };
    let mut args = vec!["commit-tree", tree.as_str()];
    for p in &parents {
        args.extend(["-p", p.as_str()]);
    }
    args.extend(["-m", message.as_str()]);
    let entry = git(f._root.path(), &args);
    git(
        f._root.path(),
        &["update-ref", "refs/heads/main", &entry, &tip],
    );
    if matches!(mode, "amend" | "squash") {
        f.input.commit = entry.clone();
    }
    f.catalog = Catalog::load(f._root.path()).unwrap();
    entry
}
#[test]
fn terminal_g5_supports_multiple_task_commits_amend_squash_and_captured_task_parents() {
    for shape in ["single", "multi", "multi_parent"] {
        for mode in ["direct", "amend", "squash"] {
            let mut f = MergeFixture::with_shape(false, shape);
            if mode != "direct" {
                replace_task_main(&mut f, mode);
            }
            let history =
                read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
            let canonical = prove_merge(&f.catalog, &history, &f.input)
                .unwrap()
                .into_event();
            assert_eq!(canonical.data["commit"], f.input.commit, "{shape}/{mode}");
            assert_eq!(
                canonical.data["proof"]["integration_gate"],
                serde_json::to_value(&f.integration_gate).unwrap()
            );
        }
    }
}
#[test]
fn actual_two_parent_entry_is_canonical_and_side_tip_cannot_bypass_bad_main_edge() {
    for mode in ["merge", "code_revert", "missing_footer"] {
        let mut f = MergeFixture::with_shape(false, "multi");
        let entry = replace_task_main(&mut f, mode);
        let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
        let side = prove_merge(&f.catalog, &history, &f.input);
        if mode == "merge" {
            let canonical = side.unwrap().into_event();
            assert_eq!(canonical.data["commit"], entry);
            assert_eq!(
                canonical.eid,
                crate::events::derived_eid(&entry, "merge.done").unwrap()
            );
            let direct = prove_merge(
                &f.catalog,
                &history,
                &MergeProofInput {
                    commit: entry,
                    trailers: f.input.trailers.clone(),
                },
            )
            .unwrap()
            .into_event();
            assert_eq!(canonical, direct);
        } else {
            assert!(side.is_err(), "{mode}: side tip bypassed actual main entry");
        }
    }
}
#[test]
fn terminal_gate_rejects_wrong_completed_parents_baseline_phase_and_original_staleness() {
    for change in [
        "completed",
        "parents",
        "baseline",
        "phase",
        "stale",
        "authority",
        "tree",
    ] {
        let f = MergeFixture::new(false);
        let mut event = f.integration_gate.clone();
        match change {
            "completed" => event.data["completed_eid"] = json!("not-original-completed"),
            "parents" => event.data["parents"] = json!([f.input.commit]),
            "baseline" => event.data["evidence"]["baseline"] = json!("towermd/B9"),
            "phase" => event.seq = f.completed.seq,
            "stale" => event.stale = true,
            "authority" => event.data["evidence"]["authority"] = json!(f.input.commit),
            "tree" => event.data["evidence"]["tree"] = json!("f".repeat(40)),
            _ => unreachable!(),
        }
        rewrite_receipt(&f.storage, &event);
        let result = f
            .outbox
            .read_inputs()
            .and_then(|inputs| read_recovery_history(&f.catalog, &inputs))
            .and_then(|history| prove_merge(&f.catalog, &history, &f.input).map(|_| ()));
        assert!(result.is_err(), "{change}");
    }
}
fn clone_task_history(f: &MergeFixture) -> (tempfile::TempDir, Catalog) {
    for branch in git(
        f._root.path(),
        &["for-each-ref", "--format=%(refname:short)", "refs/heads/"],
    )
    .lines()
    .filter(|b| *b != "main")
    {
        git(f._root.path(), &["branch", "-D", branch]);
    }
    let destination = tempfile::tempdir().unwrap();
    let path = destination.path().join("clone");
    git(
        destination.path(),
        &[
            "clone",
            "-q",
            "--no-local",
            &format!("file://{}", f._root.path().display()),
            path.to_str().unwrap(),
        ],
    );
    assert!(!path.join(".git/objects/info/alternates").exists());
    let catalog = Catalog::load(&path).unwrap();
    (destination, catalog)
}
#[test]
fn consumer_cold_clone_amend_preserves_original_freeze_and_terminal_receipts() {
    let mut f = MergeFixture::new(false);
    replace_task_main(&mut f, "amend");
    let (_clone, catalog) = clone_task_history(&f);
    let frozen: FrozenFinish = serde_json::from_value(f.started.data["input"].clone()).unwrap();
    assert!(git_bytes(&catalog, &["cat-file", "-e", &frozen.commit]).is_err());
    let history = read_recovery_history(
        &catalog,
        &RecoveryInputs::read(f.storage.path(), &catalog).unwrap(),
    )
    .unwrap();
    let canonical = prove_merge(&catalog, &history, &f.input)
        .unwrap()
        .into_event();
    assert_eq!(canonical.data["commit"], f.input.commit);
    assert_eq!(
        canonical.data["proof"]["started"],
        serde_json::to_value(f.started).unwrap()
    );
    assert_eq!(
        canonical.data["proof"]["integration_gate"],
        serde_json::to_value(f.integration_gate).unwrap()
    );
}
#[test]
fn consumer_cold_clone_state_only_rebase_uses_new_terminal_without_rerunning_g3() {
    let mut f = MergeFixture::new(false);
    let frozen: FrozenFinish = serde_json::from_value(f.started.data["input"].clone()).unwrap();
    git(f._root.path(), &["switch", "-q", "contract-task"]);
    git(
        f._root.path(),
        &[
            "update-ref",
            "refs/heads/main",
            &frozen.base,
            &f.input.commit,
        ],
    );
    git(
        f._root.path(),
        &["switch", "-qc", "state-base", &frozen.base],
    );
    let source = Catalog::load(f._root.path()).unwrap();
    let events = f.hub.events(0)[..2].to_vec();
    let snapshot =
        crate::snapshot::Snapshot::from_events(source.config.project_id(), &events, 2, 2).unwrap();
    let mut archives = BTreeMap::<String, String>::new();
    for e in &events {
        archives
            .entry(e.t[..7].into())
            .or_default()
            .push_str(&format!("{}\n", serde_json::to_string(e).unwrap()));
    }
    let request = crate::snapshot_git::WriteRequest {
        base: source.revision.clone(),
        docs_dir: source.config.docs_dir().into(),
        state_json: serde_json::to_string(&snapshot).unwrap(),
        archives,
        through_seq: 2,
        writer_epoch: 2,
    };
    let crate::snapshot_git::WriteOutcome::Written { commit: base } =
        crate::snapshot_git::write_snapshot(&source.root, &request, || Ok(())).unwrap()
    else {
        panic!()
    };
    git(f._root.path(), &["switch", "-qc", "rebased-task", &base]);
    std::fs::write(f._root.path().join("code.txt"), "task\n").unwrap();
    git(f._root.path(), &["add", "code.txt"]);
    git(
        f._root.path(),
        &[
            "commit",
            "-qm",
            &format!(
                "state-only rebase\n\n{}",
                render_task_trailers(&f.input.trailers).unwrap()
            ),
        ],
    );
    let task = git(f._root.path(), &["rev-parse", "HEAD"]);
    let tree = git(f._root.path(), &["rev-parse", "HEAD^{tree}"]);
    let capture = FrozenFinish {
        commit: task.clone(),
        tree,
        base: base.clone(),
        authority: base.clone(),
        ..frozen.clone()
    };
    terminal_fixture(
        &mut f.hub,
        &mut f.outbox,
        &Catalog::load(f._root.path()).unwrap(),
        &capture,
        &f.completed,
        None,
    );
    git(
        f._root.path(),
        &["update-ref", "refs/heads/main", &task, &base],
    );
    git(f._root.path(), &["switch", "-q", "main"]);
    f.input.commit = task;
    let (_clone, catalog) = clone_task_history(&f);
    assert!(git_bytes(&catalog, &["cat-file", "-e", &frozen.commit]).is_err());
    assert!(git_bytes(&catalog, &["cat-file", "-e", &frozen.tree]).is_err());
    let inputs = RecoveryInputs::read(f.storage.path(), &catalog).unwrap();
    assert_eq!(
        inputs
            .confirmed()
            .iter()
            .filter(|e| e.kind == "finish.completed")
            .count(),
        1
    );
    let history = read_recovery_history(&catalog, &inputs).unwrap();
    let canonical = prove_merge(&catalog, &history, &f.input)
        .unwrap()
        .into_event();
    assert_eq!(canonical.data["commit"], f.input.commit);
    assert_eq!(
        canonical.data["proof"]["completed"],
        serde_json::to_value(f.completed).unwrap()
    );
    assert_eq!(
        canonical.data["proof"]["integration_gate"]["data"]["evidence"]["base"],
        base
    );
}
#[test]
fn later_equivalent_terminal_receipt_does_not_change_canonical_event() {
    let mut f = MergeFixture::new(false);
    let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    let first = prove_merge(&f.catalog, &history, &f.input)
        .unwrap()
        .into_event();
    let mut original = crate::recovery_outbox::original_of(&f.integration_gate);
    let u = uuid::Uuid::now_v7().to_string();
    original.eid = request_event_eid(&u, "evidence.recorded").unwrap();
    original.obj = format!("E-{u}");
    original.data["evidence"]["id"] = json!(original.obj);
    append_fixture(&mut f.hub, &mut f.outbox, original);
    let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    assert_eq!(
        first,
        prove_merge(&f.catalog, &history, &f.input)
            .unwrap()
            .into_event()
    );
}
#[test]
fn merge_requires_original_terminal_g5_not_just_passed_completed() {
    let f = MergeFixture::new(false);
    for entry in std::fs::read_dir(f.storage.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|p| p.to_str()) != Some("jsonl") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines = text
            .lines()
            .filter(|line| {
                let v: Value = serde_json::from_str(line).unwrap();
                v["original"]["data"]["evidence"]["gate"] != "g5"
            })
            .collect::<Vec<_>>();
        std::fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
    }
    let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    assert!(prove_merge(&f.catalog, &history, &f.input).is_err());
}
#[test]
fn real_git_merge_requires_complete_original_pass_chain() {
    let f = MergeFixture::new(false);
    let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    let event = prove_merge(&f.catalog, &history, &f.input)
        .unwrap()
        .into_event();
    assert_eq!(event.data["task"]["state"], "merged");
    assert_eq!(event.data["run"]["state"], "ended");
    assert_eq!(event.data["task"]["x-original"]["keep"], true);
    assert!(event.guards.is_empty());
    assert_eq!(
        event.eid,
        crate::events::derived_eid(&f.input.commit, "merge.done").unwrap()
    );
    assert_eq!(
        event,
        prove_merge(&f.catalog, &history, &f.input)
            .unwrap()
            .into_event()
    );
}
struct PublicationFixture {
    root: tempfile::TempDir,
    storage: tempfile::TempDir,
    _hubdir: tempfile::TempDir,
    hub: crate::hub::Hub,
    outbox: crate::recovery_outbox::RecoveryOutbox,
    catalog: Catalog,
    commit: String,
    manifest_path: String,
}
impl PublicationFixture {
    fn new() -> Self {
        Self::with_canvas(None)
    }
    fn with_canvas(canvas: Option<&str>) -> Self {
        Self::build(canvas, None)
    }
    fn build(canvas: Option<&str>, draft_revision: Option<&str>) -> Self {
        Self::from_previous(None, canvas, draft_revision)
    }
    fn from_previous(
        previous: Option<Self>,
        canvas: Option<&str>,
        draft_revision: Option<&str>,
    ) -> Self {
        use crate::hub_protocol::*;
        use contract::baseline::*;
        let (root, c, storage, hd, mut hub, mut outbox, number) = match previous {
            Some(previous) => (
                previous.root,
                previous.catalog,
                previous.storage,
                previous._hubdir,
                previous.hub,
                previous.outbox,
                1u64,
            ),
            None => {
                let (root, c) = catalog();
                let storage = tempfile::tempdir().unwrap();
                let hd = tempfile::tempdir().unwrap();
                let hub = crate::hub::Hub::open(hd.path()).unwrap();
                let outbox = crate::recovery_outbox::RecoveryOutbox::open(
                    storage.path(),
                    crate::recovery_outbox::RecoveryContext::from_catalog(&c).unwrap(),
                )
                .unwrap();
                (root, c, storage, hd, hub, outbox, 0u64)
            }
        };
        let source = c.revision.clone();
        let target = format!("towermd/B{number}");
        let path = "docs/tower/arch/M.md";
        let text =format!("---\ntype: Module\nid: M\nlayer: L1\npaths: [code.txt]\nx-user: 原文\n---\nmodule {number}\n");
        let mut freeze = PublicationFreeze {
            project_id: c.config.project_id().into(),
            draft_id: "draft".into(),
            docs_dir: c.config.docs_dir().into(),
            source_revision: source.clone(),
            target_baseline: target.clone(),
            base_directory: publication_base_directory(&c, &source).unwrap(),
            files: BTreeMap::from([(
                path.into(),
                PublicationFile {
                    mode: "100644".into(),
                    sha256: contract::sha256(text.as_bytes()),
                },
            )]),
            deletions: vec![],
            questions: vec!["first".into(), "second".into()],
            risks: vec![],
        };
        let mut texts = BTreeMap::from([(path.into(), text.clone())]);
        if let Some(canvas) = canvas {
            let p = "docs/tower/tower.canvas";
            freeze.files.insert(
                p.into(),
                PublicationFile {
                    mode: "100644".into(),
                    sha256: contract::sha256(canvas.as_bytes()),
                },
            );
            texts.insert(p.into(), canvas.into());
        }
        let content_hash = publication_content_hash(&freeze, &texts).unwrap();
        let binding = contract::baseline::ReviewBinding {
            project_id: freeze.project_id.clone(),
            object_id: "draft".into(),
            content_hash: content_hash.clone(),
            target_baseline: target.clone(),
            draft_revision: draft_revision.unwrap_or(&source).into(),
        };
        let id = uuid::Uuid::now_v7().to_string();
        let approval_eid = request_event_eid(&id, "baseline.approved").unwrap();
        let fact = ApprovalFact {
            approval_eid: approval_eid.clone(),
            binding: binding.clone(),
            actor: "human:fixture".into(),
            verdict: Verdict::Approved,
            comment: "contract history fixture".into(),
        };
        let body = RequestBody {
            project_id: freeze.project_id.clone(),
            actor: fact.actor.clone(),
            host: "h".into(),
            run: None,
            action: BaselineReviewAction::ReviewBaseline {
                binding: binding.clone(),
                verdict: Verdict::Approved,
                comment: fact.comment.clone(),
            },
        };
        let hash = contract::canonical_hash(&body).unwrap();
        let approval_data = BaselineReviewRecorded {
            approval: fact.clone(),
            request_body: body,
            p3_request: RequestMeta {
                request_id: id.clone(),
                body_hash: hash.clone(),
            },
            p3_receipt: RequestReceipt {
                request_id: id,
                body_hash: hash,
                result_ref: fact.clone(),
            },
        };
        let main = match hub
            .subscribe(hub.contiguous_seq(), 103)
            .unwrap()
            .head
            .leases
            .get("main")
        {
            Some(lease) => lease.fence(),
            None => hub
                .lease_acquire(
                    AcquireLease {
                        host: "h".into(),
                        names: vec!["main".into()],
                        holder: "writer".into(),
                        ttl_ms: 100000,
                    },
                    103,
                )
                .unwrap()
                .leases[0]
                .fence(),
        };
        let approval = append_fixture(
            &mut hub,
            &mut outbox,
            NewEvent {
                eid: approval_eid.clone(),
                host: "h".into(),
                actor: fact.actor.clone(),
                kind: "baseline.approved".into(),
                layer: "L1".into(),
                obj: "draft".into(),
                data: serde_json::to_value(approval_data).unwrap(),
                guards: vec![],
            },
        );
        let request = uuid::Uuid::now_v7().to_string();
        let prepared_eid = request_event_eid(&request, "baseline.publication_prepared").unwrap();
        let body = RequestBody {
            project_id: freeze.project_id.clone(),
            actor: "process:towermd-baseline".into(),
            host: "h".into(),
            run: None,
            action: PublicationPrepareAction::PreparePublication {
                publication_id: approval_eid.clone(),
                approval_eid: approval_eid.clone(),
                binding: binding.clone(),
                authority_revision: source.clone(),
                content_hash: content_hash.clone(),
            },
        };
        let hash = contract::canonical_hash(&body).unwrap();
        let result = PublicationPreparedResult {
            prepared_eid: prepared_eid.clone(),
            publication_id: approval_eid.clone(),
            authority_revision: source.clone(),
            binding: binding.clone(),
            content_hash: content_hash.clone(),
        };
        let data = PublicationPrepared {
            publication_id: approval_eid.clone(),
            approval_eid: approval_eid.clone(),
            binding: binding.clone(),
            authority_revision: source.clone(),
            content_hash: content_hash.clone(),
            request_body: body,
            p3_request: RequestMeta {
                request_id: request.clone(),
                body_hash: hash.clone(),
            },
            p3_receipt: RequestReceipt {
                request_id: request,
                body_hash: hash,
                result_ref: result,
            },
        };
        let prepared = append_fixture(
            &mut hub,
            &mut outbox,
            NewEvent {
                eid: prepared_eid,
                host: "h".into(),
                actor: "process:towermd-baseline".into(),
                kind: "baseline.publication_prepared".into(),
                layer: "L1".into(),
                obj: approval_eid.clone(),
                data: serde_json::to_value(data).unwrap(),
                guards: vec![main.clone()],
            },
        );
        let (decision_path, manifest_path) =
            publication_paths(c.config.docs_dir(), &target, &approval_eid).unwrap();
        let decision = json!({"type":"Decision","id":format!("publication-{}",contract::sha256(approval_eid.as_bytes())),"layer":"L1","context":format!("baseline publication {target}"),"options":["approve","reject"],"decision":"approve","rationale":fact.comment,"affects":["M"],"supersedes":[],"class":"baseline-publication","generated":{"by":fact.actor,"at":approval.t},"verified":[{"by":fact.actor,"at":approval.t}],"publication":{"schema":1,"publication_id":approval_eid,"approval":fact,"approval_event":approval}});
        let manifest = json!({"type":"Baseline","id":format!("B{number}"),"layer":"L1","objects":{"M":contract::sha256(text.as_bytes())},"approved_by":fact.actor,"approved_at":approval.t,"git_tag":target,"generated":{"by":"process:towermd-baseline","at":approval.t},"publication":{"schema":1,"publication_id":approval_eid,"approval_eid":approval_eid,"decision_path":decision_path,"parent_baseline":number.checked_sub(1).map(|n|format!("towermd/B{n}")),"publication_host":"h","writer_epoch":main.epoch,"prepared_event":prepared,"binding":binding,"freeze":freeze}});
        for (p, v) in [(&decision_path, decision), (&manifest_path, manifest)] {
            let p = root.path().join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(
                p,
                format!("---\n{}---\n", serde_yaml::to_string(&v).unwrap()),
            )
            .unwrap();
        }
        std::fs::create_dir_all(root.path().join("docs/tower/arch")).unwrap();
        std::fs::write(root.path().join(path), &text).unwrap();
        if let Some(canvas) = canvas {
            std::fs::write(root.path().join("docs/tower/tower.canvas"), canvas).unwrap();
        }
        git(root.path(), &["add", "."]);
        git(root.path(),&["commit","-qm",&format!("actual approved publication\n\nTowermd-Baseline: {target}\nTowermd-Approval: {approval_eid}\nTowermd-Publication: {approval_eid}\nTowermd-Content-Hash: {content_hash}\nTowermd-Epoch: {}",main.epoch)]);
        let commit = git(root.path(), &["rev-parse", "HEAD"]);
        git(
            root.path(),
            &["tag", "-a", &target, "-m", "approved baseline"],
        );
        let catalog = Catalog::load(root.path()).unwrap();
        Self {
            root,
            storage,
            _hubdir: hd,
            hub,
            outbox,
            catalog,
            commit,
            manifest_path,
        }
    }
}
fn amend_publication(f: &mut PublicationFixture, path: &str, edit: impl FnOnce(&mut Value)) {
    let file = f.root.path().join(path);
    let mut value = front(&std::fs::read_to_string(&file).unwrap()).unwrap();
    edit(&mut value);
    std::fs::write(
        file,
        format!("---\n{}---\n", serde_yaml::to_string(&value).unwrap()),
    )
    .unwrap();
    git(f.root.path(), &["add", "."]);
    git(f.root.path(), &["commit", "--amend", "--no-edit", "-q"]);
    f.commit = git(f.root.path(), &["rev-parse", "HEAD"]);
    let target = format!(
        "towermd/{}",
        std::path::Path::new(&f.manifest_path)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
    );
    git(f.root.path(), &["tag", "-f", &target]);
    f.catalog = Catalog::load(f.root.path()).unwrap();
}
#[test]
fn publication_proves_complete_parent_chain_and_diagnoses_extra_tags() {
    let first = PublicationFixture::new();
    let f = PublicationFixture::from_previous(Some(first), None, None);
    let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    let proof = prove_publication(&f.catalog, &history, &f.commit).unwrap();
    assert_eq!(
        baseline_created_event(&proof).unwrap().data["target"],
        "towermd/B1"
    );
    git(f.root.path(), &["tag", "towermd/B99"]);
    git(f.root.path(), &["tag", "towermd/Bbad"]);
    let proof = prove_publication(&f.catalog, &history, &f.commit).unwrap();
    assert_eq!(proof.diagnostics().len(), 2);
    assert_eq!(
        baseline_created_event(&proof).unwrap().data["tags"],
        json!(["towermd/B1"])
    );
    git(f.root.path(), &["tag", "-d", "towermd/B0"]);
    assert!(prove_publication(&f.catalog, &history, &f.commit).is_err());
}
#[test]
fn publication_parent_chain_rejects_conflicting_original_receipt_coordinates() {
    let first = PublicationFixture::new();
    let mut f = PublicationFixture::from_previous(Some(first), None, None);
    let meta = read_publication_metadata(&f.catalog, &f.commit).unwrap();
    let path = meta.decision_path;
    amend_publication(&mut f, &path, |v| {
        v["publication"]["approval_event"]["seq"] = json!(3)
    });
    let history = read_recovery_history(
        &f.catalog,
        &RecoveryInputs::read(&f.storage.path().join("absent"), &f.catalog).unwrap(),
    )
    .unwrap();
    assert!(prove_publication(&f.catalog, &history, &f.commit).is_err());
}
#[test]
fn publication_requires_well_formed_original_draft_revision() {
    let f = PublicationFixture::build(None, Some(""));
    let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    assert!(prove_publication(&f.catalog, &history, &f.commit).is_err());
}
#[test]
fn publication_rejects_malformed_binding_preparation_payload_and_scope() {
    for (pointer, bad) in [
        ("/publication/binding/draft_revision", json!("")),
        ("/publication/prepared_event/stale", json!(true)),
        (
            "/publication/prepared_event/guards/0/name",
            json!("module:M"),
        ),
        (
            "/publication/freeze/files/docs~1tower~1arch~1M.md/sha256",
            json!("f".repeat(64)),
        ),
        ("/publication/parent_baseline", json!("towermd/B7")),
        ("/publication/schema", json!(99)),
        ("/objects/M", json!("f".repeat(64))),
    ] {
        let mut f = PublicationFixture::new();
        let path = f.manifest_path.clone();
        amend_publication(&mut f, &path, |v| *v.pointer_mut(pointer).unwrap() = bad);
        let history = read_recovery_history(
            &f.catalog,
            &RecoveryInputs::read(&f.storage.path().join("empty"), &f.catalog).unwrap(),
        )
        .unwrap();
        assert!(
            prove_publication(&f.catalog, &history, &f.commit).is_err(),
            "{pointer}"
        );
    }
}
#[test]
fn publication_binds_prepared_request_host_to_original_event() {
    let mut f = PublicationFixture::new();
    let path = f.manifest_path.clone();
    amend_publication(&mut f, &path, |value| {
        let event = &mut value["publication"]["prepared_event"];
        event["data"]["request_body"]["host"] = json!("other-host");
        let hash = contract::canonical_hash(&event["data"]["request_body"]).unwrap();
        event["data"]["p3_request"]["body_hash"] = json!(hash);
        event["data"]["p3_receipt"]["body_hash"] = json!(hash);
    });
    let inputs = RecoveryInputs::read(&f.storage.path().join("absent"), &f.catalog).unwrap();
    let history = read_recovery_history(&f.catalog, &inputs).unwrap();
    assert!(prove_publication(&f.catalog, &history, &f.commit).is_err());
}
#[test]
fn publication_rejects_duplicate_canvas_keys_and_missing_object_references() {
    for canvas in [
        r#"{"nodes":[],"nodes":[],"edges":[]}"#,
        r#"{"nodes":[{"id":"n","type":"file","file":"missing.md","x":0,"y":0,"width":100,"height":100,"towermd":{"obj":"missing"}}],"edges":[]}"#,
    ] {
        let f = PublicationFixture::with_canvas(Some(canvas));
        let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
        assert!(
            prove_publication(&f.catalog, &history, &f.commit).is_err(),
            "{canvas}"
        );
    }
}
#[test]
fn second_recovery_rechecks_publication_from_actual_commit_without_original_outbox() {
    let f = PublicationFixture::new();
    let inputs = f.outbox.read_inputs().unwrap();
    let history = read_recovery_history(&f.catalog, &inputs).unwrap();
    let canonical =
        baseline_created_event(&prove_publication(&f.catalog, &history, &f.commit).unwrap())
            .unwrap();
    persist_recovered_archive(
        &f.catalog,
        f.hub.events(0)[..1].to_vec(),
        inputs.confirmed(),
        inputs.high_water(),
        &canonical,
    );
    drop(f.outbox);
    drop(f.hub);
    std::fs::remove_dir_all(f.storage.path()).unwrap();
    let inputs = RecoveryInputs::read(f.storage.path(), &f.catalog).unwrap();
    let history = read_recovery_history(&f.catalog, &inputs).unwrap();
    assert_eq!(
        canonical,
        baseline_created_event(&prove_publication(&f.catalog, &history, &f.commit).unwrap())
            .unwrap()
    );
    assert!(!f.storage.path().exists());
}
#[test]
fn publication_requires_actual_exact_payload_original_preparation_and_tag() {
    let mut f = PublicationFixture::new();
    let inputs = RecoveryInputs::read(f.storage.path(), &f.catalog).unwrap();
    let history = read_recovery_history(&f.catalog, &inputs).unwrap();
    let event =
        baseline_created_event(&prove_publication(&f.catalog, &history, &f.commit).unwrap())
            .unwrap();
    assert_eq!(event.data["target"], "towermd/B0");
    assert_eq!(event.host, "h");
    assert!(event.guards.is_empty());
    append_fixture(&mut f.hub, &mut f.outbox, event.clone());
    let history = read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap();
    assert_eq!(
        event,
        baseline_created_event(&prove_publication(&f.catalog, &history, &f.commit).unwrap())
            .unwrap()
    );
    git(f.root.path(), &["tag", "-d", "towermd/B0"]);
    let error = prove_publication(&f.catalog, &history, &f.commit)
        .err()
        .map(|e| e.to_string())
        .unwrap();
    assert!(error.contains("pending publication"), "{error}");
}
#[test]
fn canonical_history_rejects_bare_baseline_created_claim() {
    let (_root, c) = catalog();
    let storage = tempfile::tempdir().unwrap();
    let hd = tempfile::tempdir().unwrap();
    let mut hub = crate::hub::Hub::open(hd.path()).unwrap();
    let mut outbox = crate::recovery_outbox::RecoveryOutbox::open(
        storage.path(),
        crate::recovery_outbox::RecoveryContext::from_catalog(&c).unwrap(),
    )
    .unwrap();
    append_fixture(
        &mut hub,
        &mut outbox,
        NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "h".into(),
            actor: "process:towermd-baseline".into(),
            kind: "baseline.created".into(),
            layer: "L1".into(),
            obj: "approval".into(),
            data: json!({"commit":c.revision,"target":"towermd/B0"}),
            guards: vec![],
        },
    );
    assert!(read_recovery_history(&c, &outbox.read_inputs().unwrap()).is_err());
}
#[test]
fn canonical_history_rejects_unproved_self_reported_merge() {
    let (_root, c) = catalog();
    let storage = tempfile::tempdir().unwrap();
    let hd = tempfile::tempdir().unwrap();
    let mut hub = crate::hub::Hub::open(hd.path()).unwrap();
    let mut outbox = crate::recovery_outbox::RecoveryOutbox::open(
        storage.path(),
        crate::recovery_outbox::RecoveryContext::from_catalog(&c).unwrap(),
    )
    .unwrap();
    let event = NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "h".into(),
        actor: "process:towermd-integrator".into(),
        kind: "merge.done".into(),
        layer: "L4".into(),
        obj: "T".into(),
        data: json!({"verified":true,"commit":c.revision}),
        guards: vec![],
    };
    crate::recovery_outbox::append_durable(&mut hub, &mut outbox, event, 100).unwrap();
    assert!(read_recovery_history(&c, &outbox.read_inputs().unwrap()).is_err());
}
#[test]
fn mixed_or_duplicate_known_trailers_fail_and_unknown_footer_lines_remain_exact() {
    let snapshot="title\r\n\r\nTowermd-Snapshot: 7\r\nTowermd-Epoch: 2\r\nTowermd-Future: raw value\r\nTowermd-Other: x:y\r\n";
    assert_eq!(
        parse_trailer_context(snapshot).unwrap(),
        TrailerContext::Snapshot { seq: 7, epoch: 2 }
    );
    assert_eq!(
        unknown_trailer_lines(snapshot),
        vec!["Towermd-Future: raw value", "Towermd-Other: x:y"]
    );
    for bad in [
        "title\n\nTowermd-Epoch: 2",
        "title\n\nTowermd-Snapshot: 7\nTowermd-Epoch: 2\nTowermd-Task: T",
        "title\n\nTowermd-Snapshot: 7\nTowermd-Epoch: 2\nTowermd-Epoch: 2",
    ] {
        assert!(parse_trailer_context(bad).is_err(), "{bad}");
    }
}
#[test]
fn task_epoch_is_not_a_snapshot_and_requires_all_five_trailers() {
    let text = "task\n\nTowermd-Task: T\nTowermd-Run: R\nTowermd-Module: M\nTowermd-Evidence: E\nTowermd-Epoch: 7\n";
    assert_eq!(
        parse_trailer_context(text).unwrap(),
        TrailerContext::Task(TaskTrailers {
            task: "T".into(),
            run: "R".into(),
            module: "M".into(),
            evidence: "E".into(),
            epoch: 7
        })
    );
    assert!(parse_trailer_context(&text.replace("Towermd-Epoch: 7\n", "")).is_err());
    assert!(parse_trailer_context(&format!("{text}Towermd-Task: T\n")).is_err());
    assert_eq!(
        parse_trailer_context("Towermd-Task: T\n\nordinary footer").unwrap(),
        TrailerContext::None
    );
}

#[test]
fn consumer_rejects_corrupt_existing_frozen_object_in_cold_clone() {
    let mut f = MergeFixture::new(false);
    replace_task_main(&mut f, "amend");
    let (_clone, catalog) = clone_task_history(&f);
    let frozen: FrozenFinish = serde_json::from_value(f.started.data["input"].clone()).unwrap();
    assert!(git_bytes(&catalog, &["cat-file", "-e", &frozen.commit]).is_err());
    let relative = git(
        &catalog.root,
        &[
            "rev-parse",
            "--git-path",
            &format!("objects/{}/{}", &frozen.commit[..2], &frozen.commit[2..]),
        ],
    );
    let object = catalog.root.join(relative);
    std::fs::create_dir_all(object.parent().unwrap()).unwrap();
    std::fs::write(&object, b"exists but corrupt zlib object").unwrap();
    let actual = crate::repo::git_timeout_env_output(
        &catalog.root,
        &["cat-file", "-t", &frozen.commit],
        std::time::Duration::from_secs(10),
        &[],
    )
    .unwrap();
    eprintln!(
        "corrupt exists={} status={:?} stderr={}",
        object.exists(),
        actual.status.code(),
        String::from_utf8_lossy(&actual.stderr)
    );
    let history = read_recovery_history(
        &catalog,
        &RecoveryInputs::read(f.storage.path(), &catalog).unwrap(),
    )
    .unwrap();
    assert!(
        prove_merge(&catalog, &history, &f.input).is_err(),
        "corrupt object must not become missing-object fallback"
    );
}

#[test]
fn independent_system_receipts_never_relax_business_or_same_eid_conflicts() {
    let f = MergeFixture::new(false);
    let mut events = BTreeMap::new();
    let lease = f
        .hub
        .events(0)
        .into_iter()
        .find(|e| e.kind.starts_with("lease."))
        .unwrap();
    insert_original(&mut events, lease.clone()).unwrap();
    let mut business = f.claim.clone();
    business.seq = lease.seq;
    insert_original(&mut events, business.clone()).unwrap();
    let mut conflicting = business;
    conflicting.eid = uuid::Uuid::now_v7().to_string();
    assert!(insert_original(&mut events, conflicting).is_err());
    let mut changed = lease;
    changed.data = json!({"different":"system body"});
    assert!(insert_original(&mut events, changed).is_err());
}

// Contract history fixture only: production recovery consumes real original receipts and Git.
fn seed_snapshot_before_merge(f: &mut MergeFixture) {
    use crate::hub_protocol::HubProtocol;
    let frozen: FrozenFinish = serde_json::from_value(f.started.data["input"].clone()).unwrap();
    git(
        &f.catalog.root,
        &[
            "update-ref",
            "refs/heads/main",
            &frozen.base,
            &f.input.commit,
        ],
    );
    let catalog = Catalog::load(&f.catalog.root).unwrap();
    let prefix: Vec<_> = f
        .hub
        .subscribe(0, 104)
        .unwrap()
        .events
        .into_iter()
        .take_while(|e| e.seq < f.claim.seq)
        .collect();
    let snapshot = crate::snapshot::Snapshot::from_events(
        catalog.config.project_id(),
        &prefix,
        prefix.last().unwrap().seq,
        2,
    )
    .unwrap();
    let mut archives = BTreeMap::<String, String>::new();
    for event in prefix {
        archives
            .entry(event.t[..7].into())
            .or_default()
            .push_str(&format!("{}\n", serde_json::to_string(&event).unwrap()));
    }
    let request = crate::snapshot_git::WriteRequest {
        base: catalog.revision.clone(),
        docs_dir: catalog.config.docs_dir().into(),
        state_json: serde_json::to_string(&snapshot).unwrap(),
        archives,
        through_seq: snapshot.through_seq,
        writer_epoch: snapshot.writer_epoch,
    };
    assert!(matches!(
        crate::snapshot_git::write_snapshot(&catalog.root, &request, || Ok(())).unwrap(),
        crate::snapshot_git::WriteOutcome::Written { .. }
    ));
    git(
        &catalog.root,
        &[
            "update-ref",
            "refs/heads/main",
            &f.input.commit,
            &frozen.base,
        ],
    );
    f.catalog = Catalog::load(&catalog.root).unwrap();
}

#[test]
fn confirmed_claim_freeze_pass_restore_git_merge_after_hub_loss() {
    for high in [false, true] {
        let mut f = MergeFixture::with_shape(high, "consumer");
        seed_snapshot_before_merge(&mut f);
        let dependent = json!({"type":"Task","id":"D","title":"dependent","module":"M","touches":[],"allowed_paths":["code.txt"],"depends_on":["T"],"acceptance":["AC"],"decisions":[],"state":"proposed"});
        f.outbox
            .append_pending(&NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "h".into(),
                actor: "process:towermd-cli".into(),
                kind: "task.state".into(),
                layer: "L4".into(),
                obj: "D".into(),
                data: json!({"task":dependent}),
                guards: vec![],
            })
            .unwrap();
        let inputs = f.outbox.read_inputs().unwrap();
        let history = read_recovery_history(&f.catalog, &inputs).unwrap();
        let canonical = prove_merge(&f.catalog, &history, &f.input)
            .unwrap()
            .into_event();
        let request = crate::recovery::prepare(
            &f.catalog,
            &inputs,
            1000,
            10000,
            &uuid::Uuid::now_v7().to_string(),
        )
        .unwrap();
        let (_, session) = crate::snapshot_service::SnapshotWriter::default()
            .confirm_rebuild(
                None,
                &f.catalog,
                &request,
                "human:fixture",
                10000,
                &inputs,
                "h",
                || Ok(2000),
            )
            .unwrap();
        let current = Catalog::load(&f.catalog.root).unwrap();
        let projection = session.lock().unwrap().projection().unwrap().clone();
        assert_eq!(projection.objects["Task"]["T"]["state"], "merged");
        assert_eq!(projection.objects["Run"]["R"]["state"], "ended");
        assert_eq!(projection.objects["Evidence"]["E"]["id"], "E");
        assert_eq!(projection.objects["Task"]["D"]["state"], "proposed");
        let saved = crate::snapshot_service::read_snapshot(&current)
            .unwrap()
            .unwrap();
        let records: Vec<Event> = saved
            .archives
            .values()
            .flat_map(|text| text.lines().map(|line| serde_json::from_str(line).unwrap()))
            .collect();
        let event = records.iter().find(|e| e.kind == "merge.done").unwrap();
        assert_eq!(crate::recovery_outbox::original_of(event), canonical);
        assert!(
            !records
                .iter()
                .filter(|e| e.kind == "alert.raised")
                .any(|e| {
                    e.data["raw_lines"].as_array().is_some_and(|lines| {
                        lines.iter().any(|line| {
                            line.as_str().is_some_and(|line| {
                                line.starts_with("Towermd-Run:")
                                    || line.starts_with("Towermd-Module:")
                            })
                        })
                    })
                }),
            "known Task companion trailers must not produce unknown alerts"
        );
        assert!(records.iter().any(|e| e.eid == f.claim.eid && e.stale));
        assert!(!records
            .iter()
            .any(|e| e.kind == "task.released" && e.obj == "T"));
        // No second confirmed record is emitted by internal staging replay.
        assert_eq!(f.outbox.read_inputs().unwrap().digest(), inputs.digest());
        drop(session);
        std::fs::remove_dir_all(current.common_dir.join("towermd")).unwrap();
        drop(f.outbox);
        for entry in std::fs::read_dir(f.storage.path()).unwrap() {
            std::fs::remove_file(entry.unwrap().path()).unwrap();
        }
        let empty =
            crate::recovery_outbox::RecoveryInputs::read(f.storage.path(), &current).unwrap();
        let second = crate::recovery::prepare(
            &current,
            &empty,
            2100,
            10000,
            &uuid::Uuid::now_v7().to_string(),
        )
        .unwrap();
        let (_, restored) = crate::snapshot_service::SnapshotWriter::default()
            .confirm_rebuild(
                None,
                &current,
                &second,
                "human:fixture",
                10000,
                &empty,
                "h",
                || Ok(2200),
            )
            .unwrap();
        assert_eq!(
            restored.lock().unwrap().projection().unwrap().objects["Task"]["T"]["state"],
            "merged"
        );
        git(&current.root, &["switch", "-qc", "t/D"]);
        let claim_catalog = Catalog::load(&current.root).unwrap();
        let claim_inputs =
            crate::recovery_outbox::RecoveryInputs::read(f.storage.path(), &claim_catalog).unwrap();
        let before = restored.lock().unwrap().applied_seq();
        crate::project_session::ProjectSession::execute_shared(
            &restored,
            &claim_catalog,
            &claim_inputs,
            "h",
            crate::tasks::TaskCommand::Claim {
                id: "D".into(),
                harness: None,
                harness_version: None,
            },
            None,
            2300,
        )
        .unwrap();
        let events = restored.lock().unwrap().events(before);
        let ready = events
            .iter()
            .position(|e| {
                e.kind == "task.state" && e.obj == "D" && e.data["task"]["state"] == "ready"
            })
            .unwrap();
        let claimed = events
            .iter()
            .position(|e| e.kind == "task.claimed" && e.obj == "D")
            .unwrap();
        assert!(ready < claimed);
    }
}

fn recover_after_original_guardless_state(confirmed: bool) {
    let high = false;
    {
        let mut f = MergeFixture::with_shape(high, "consumer");
        seed_snapshot_before_merge(&mut f);
        let mut producer = crate::tasks::TaskService::default();
        producer
            .execute(
                &mut f.hub,
                &f.catalog,
                "h",
                crate::tasks::TaskCommand::Add {
                    task: crate::tasks::TaskSpec {
                        id: "T".into(),
                        title: "old original Add".into(),
                        module: "M".into(),
                        touches: vec![],
                        allowed_paths: vec!["code.txt".into()],
                        depends_on: vec![],
                        acceptance: vec!["AC".into()],
                        decisions: vec![],
                    },
                },
                None,
                104,
            )
            .unwrap();
        let accepted = f.hub.events(0).last().unwrap().clone();
        assert_eq!(accepted.kind, "task.state");
        assert_eq!(accepted.data["task"]["state"], "ready");
        assert!(!accepted.stale && accepted.guards.is_empty());
        let original = crate::recovery_outbox::original_of(&accepted);
        f.outbox.append_pending(&original).unwrap();
        if confirmed {
            f.outbox.persist_confirmed(&original, &accepted).unwrap();
        }
        drop(f.outbox);
        f.outbox = crate::recovery_outbox::RecoveryOutbox::open(
            f.storage.path(),
            crate::recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
        )
        .unwrap();
        let dependent = json!({"type":"Task","id":"D","title":"dependent","module":"M","touches":[],"allowed_paths":["code.txt"],"depends_on":["T"],"acceptance":["AC"],"decisions":[],"state":"proposed"});
        f.outbox
            .append_pending(&NewEvent {
                eid: uuid::Uuid::now_v7().to_string(),
                host: "h".into(),
                actor: "process:towermd-cli".into(),
                kind: "task.state".into(),
                layer: "L4".into(),
                obj: "D".into(),
                data: json!({"task":dependent}),
                guards: vec![],
            })
            .unwrap();
        let inputs = f.outbox.read_inputs().unwrap();
        let history = read_recovery_history(&f.catalog, &inputs).unwrap();
        let canonical = prove_merge(&f.catalog, &history, &f.input)
            .unwrap()
            .into_event();
        let request = crate::recovery::prepare(
            &f.catalog,
            &inputs,
            1000,
            10000,
            &uuid::Uuid::now_v7().to_string(),
        )
        .unwrap();
        let (_, session) = crate::snapshot_service::SnapshotWriter::default()
            .confirm_rebuild(
                None,
                &f.catalog,
                &request,
                "human:fixture",
                10000,
                &inputs,
                "h",
                || Ok(2000),
            )
            .unwrap();
        let current = Catalog::load(&f.catalog.root).unwrap();
        let projection = session.lock().unwrap().projection().unwrap().clone();
        assert_eq!(projection.objects["Task"]["T"]["state"], "merged");
        assert_eq!(projection.objects["Run"]["R"]["state"], "ended");
        assert_eq!(projection.objects["Evidence"]["E"]["id"], "E");
        assert_eq!(projection.objects["Task"]["D"]["state"], "proposed");
        let business = crate::project_session::ProjectSession::execute_shared(
            &session,
            &current,
            &inputs,
            "h",
            crate::tasks::TaskCommand::Status,
            None,
            2001,
        )
        .unwrap();
        assert_eq!(
            business["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == "T")
                .unwrap()["state"],
            "merged"
        );
        assert_eq!(
            business["runs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["id"] == "R")
                .unwrap()["state"],
            "ended"
        );
        let saved = crate::snapshot_service::read_snapshot(&current)
            .unwrap()
            .unwrap();
        let records: Vec<Event> = saved
            .archives
            .values()
            .flat_map(|text| text.lines().map(|line| serde_json::from_str(line).unwrap()))
            .collect();
        let audit = records.iter().find(|e| e.eid == original.eid).unwrap();
        assert_eq!(crate::recovery_outbox::original_of(audit), original);
        assert!(!audit.stale);
        let event = records.iter().find(|e| e.kind == "merge.done").unwrap();
        assert_eq!(crate::recovery_outbox::original_of(event), canonical);
        assert!(
            !records
                .iter()
                .filter(|e| e.kind == "alert.raised")
                .any(|e| {
                    e.data["raw_lines"].as_array().is_some_and(|lines| {
                        lines.iter().any(|line| {
                            line.as_str().is_some_and(|line| {
                                line.starts_with("Towermd-Run:")
                                    || line.starts_with("Towermd-Module:")
                            })
                        })
                    })
                }),
            "known Task companion trailers must not produce unknown alerts"
        );
        assert!(records.iter().any(|e| e.eid == f.claim.eid && e.stale));
        assert!(!records
            .iter()
            .any(|e| e.kind == "task.released" && e.obj == "T"));
        // No second confirmed record is emitted by internal staging replay.
        assert_eq!(f.outbox.read_inputs().unwrap().digest(), inputs.digest());
        drop(session);
        std::fs::remove_dir_all(current.common_dir.join("towermd")).unwrap();
        drop(f.outbox);
        for entry in std::fs::read_dir(f.storage.path()).unwrap() {
            std::fs::remove_file(entry.unwrap().path()).unwrap();
        }
        let empty =
            crate::recovery_outbox::RecoveryInputs::read(f.storage.path(), &current).unwrap();
        let second = crate::recovery::prepare(
            &current,
            &empty,
            2100,
            10000,
            &uuid::Uuid::now_v7().to_string(),
        )
        .unwrap();
        let (_, restored) = crate::snapshot_service::SnapshotWriter::default()
            .confirm_rebuild(
                None,
                &current,
                &second,
                "human:fixture",
                10000,
                &empty,
                "h",
                || Ok(2200),
            )
            .unwrap();
        assert_eq!(
            restored.lock().unwrap().projection().unwrap().objects["Task"]["T"]["state"],
            "merged"
        );
        git(&current.root, &["switch", "-qc", "t/D"]);
        let claim_catalog = Catalog::load(&current.root).unwrap();
        let claim_inputs =
            crate::recovery_outbox::RecoveryInputs::read(f.storage.path(), &claim_catalog).unwrap();
        let before = restored.lock().unwrap().applied_seq();
        crate::project_session::ProjectSession::execute_shared(
            &restored,
            &claim_catalog,
            &claim_inputs,
            "h",
            crate::tasks::TaskCommand::Claim {
                id: "D".into(),
                harness: None,
                harness_version: None,
            },
            None,
            2300,
        )
        .unwrap();
        let events = restored.lock().unwrap().events(before);
        let ready = events
            .iter()
            .position(|e| {
                e.kind == "task.state" && e.obj == "D" && e.data["task"]["state"] == "ready"
            })
            .unwrap();
        let claimed = events
            .iter()
            .position(|e| e.kind == "task.claimed" && e.obj == "D")
            .unwrap();
        assert!(ready < claimed);
    }
}

#[test]
fn recovered_merge_survives_reopened_pending_original_add() {
    recover_after_original_guardless_state(false);
}
#[test]
fn recovered_merge_survives_reopened_confirmed_original_add() {
    recover_after_original_guardless_state(true);
}

#[test]
fn recovery_invalid_original_finish_chain_never_merges_or_revives_old_run() {
    for fault in [
        "unknown-evidence",
        "failed",
        "wrong-actor",
        "wrong-tree",
        "missing-claim",
        "wrong-touch",
        "pending-only",
    ] {
        let mut f = MergeFixture::with_shape(false, "consumer");
        // AC is a real committed Acceptance. D is accepted into the original
        // Hub before the claim and is part of the actual source checkpoint.
        let (through, _, _) = matrix_reissue_history(&mut f, "valid", None);
        matrix_seed_prefix(&mut f, through);
        match fault {
            "unknown-evidence" => {
                f.completed.data["evidence"]["id"] = json!("E-other");
                rewrite_receipt(&f.storage, &f.completed);
            }
            "failed" => {
                f.completed.data["evidence"]["result"] = json!("failed");
                rewrite_receipt(&f.storage, &f.completed);
            }
            "wrong-actor" => {
                f.completed.actor = "process:self-reported".into();
                rewrite_receipt(&f.storage, &f.completed);
            }
            "wrong-tree" => {
                f.completed.data["evidence"]["tree"] = json!("b".repeat(40));
                rewrite_receipt(&f.storage, &f.completed);
            }
            "wrong-touch" => {
                f.claim.data["task"]["touches"] = json!(["unheld-module"]);
                rewrite_receipt(&f.storage, &f.claim);
            }
            "missing-claim" | "pending-only" => {
                for entry in std::fs::read_dir(f.storage.path()).unwrap() {
                    let path = entry.unwrap().path();
                    if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                        continue;
                    }
                    let text = std::fs::read_to_string(&path).unwrap();
                    let kept: Vec<_> = text
                        .lines()
                        .filter(|line| {
                            let row: Value = serde_json::from_str(line).unwrap();
                            if fault == "pending-only" {
                                row["state"] == "pending"
                            } else {
                                row["original"]["eid"] != f.claim.eid
                            }
                        })
                        .collect();
                    std::fs::write(path, format!("{}\n", kept.join("\n"))).unwrap();
                }
            }
            _ => unreachable!(),
        }
        let inputs = f.outbox.read_inputs().unwrap();
        let request = crate::recovery::prepare(
            &f.catalog,
            &inputs,
            1000,
            10000,
            &uuid::Uuid::now_v7().to_string(),
        )
        .unwrap();
        let (_, session) = crate::snapshot_service::SnapshotWriter::default()
            .confirm_rebuild(
                None,
                &f.catalog,
                &request,
                "human:fixture",
                10000,
                &inputs,
                "h",
                || Ok(2000),
            )
            .unwrap();
        let projection = session.lock().unwrap().projection().unwrap().clone();
        assert!(
            projection
                .objects
                .get("Task")
                .and_then(|t| t.get("T"))
                .is_none_or(|t| t["state"] != "merged"),
            "{fault}"
        );
        assert!(
            projection
                .objects
                .get("Run")
                .and_then(|r| r.get("R"))
                .is_none_or(|r| r["state"] != "running"),
            "{fault}"
        );
        assert_eq!(
            projection.objects["Task"]["D"]["state"], "proposed",
            "{fault}"
        );
        assert_eq!(
            projection.objects["Task"]["D"]["depends_on"],
            json!(["T"]),
            "{fault}"
        );
        assert_eq!(
            projection.objects["Task"]["D"]["acceptance"],
            json!(["AC"]),
            "{fault}"
        );
        let current = Catalog::load(&f.catalog.root).unwrap();
        let saved = crate::snapshot_service::read_snapshot(&current)
            .unwrap()
            .unwrap();
        let records: Vec<Event> = saved
            .archives
            .values()
            .flat_map(|text| text.lines().map(|line| serde_json::from_str(line).unwrap()))
            .collect();
        assert!(!records.iter().any(|e| e.kind == "merge.done"), "{fault}");
        assert!(records.iter().any(|e| e.kind == "gate.failed"), "{fault}");
        assert!(records.iter().any(|e| e.kind == "hold.opened"), "{fault}");
        matrix_claim_dependent(&session, &current.root, f.storage.path(), false, 2100);
        eprintln!("recovery {fault}: Task not merged, Run not running, D proposed; actual G0 refused ready/claim");
    }
}

#[test]
fn actual_recovery_rechecks_approved_publication_and_bare_tags_without_old_outbox() {
    use crate::hub_protocol::HubProtocol;
    let mut f = PublicationFixture::new();
    let parent = git(&f.catalog.root, &["rev-parse", &format!("{}^1", f.commit)]);
    git(
        &f.catalog.root,
        &["update-ref", "refs/heads/main", &parent, &f.commit],
    );
    let base = Catalog::load(&f.catalog.root).unwrap();
    let prefix: Vec<_> = f
        .hub
        .subscribe(0, 104)
        .unwrap()
        .events
        .into_iter()
        .take(1)
        .collect();
    let checkpoint =
        crate::snapshot::Snapshot::from_events(base.config.project_id(), &prefix, 1, 1).unwrap();
    let archives = BTreeMap::from([(
        prefix[0].t[..7].to_owned(),
        format!("{}\n", serde_json::to_string(&prefix[0]).unwrap()),
    )]);
    assert!(matches!(
        crate::snapshot_git::write_snapshot(
            &base.root,
            &crate::snapshot_git::WriteRequest {
                base: base.revision.clone(),
                docs_dir: base.config.docs_dir().into(),
                state_json: serde_json::to_string(&checkpoint).unwrap(),
                archives,
                through_seq: 1,
                writer_epoch: 1
            },
            || Ok(())
        )
        .unwrap(),
        crate::snapshot_git::WriteOutcome::Written { .. }
    ));
    git(
        &base.root,
        &["update-ref", "refs/heads/main", &f.commit, &parent],
    );
    git(&base.root, &["tag", "towermd/B99", &parent]);
    let orphan_tree = git(
        &base.root,
        &["rev-parse", &format!("{}^{{tree}}", f.commit)],
    );
    let orphan = git(
        &base.root,
        &[
            "commit-tree",
            &orphan_tree,
            "-m",
            "unreachable baseline candidate",
        ],
    );
    git(&base.root, &["tag", "towermd/B100", &orphan]);
    assert_eq!(
        git(&base.root, &["rev-parse", "towermd/B100^{commit}"]),
        orphan
    );
    assert!(!git(&base.root, &["rev-list", &f.commit])
        .lines()
        .any(|commit| commit == orphan));
    f.catalog = Catalog::load(&f.catalog.root).unwrap();
    let input = f.outbox.read_inputs().unwrap();
    let history = read_recovery_history(&f.catalog, &input).unwrap();
    let canonical =
        baseline_created_event(&prove_publication(&f.catalog, &history, &f.commit).unwrap())
            .unwrap();
    let request = crate::recovery::prepare(
        &f.catalog,
        &input,
        1000,
        10000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let (_, session) = crate::snapshot_service::SnapshotWriter::default()
        .confirm_rebuild(
            None,
            &f.catalog,
            &request,
            "human:fixture",
            10000,
            &input,
            "h",
            || Ok(2000),
        )
        .unwrap();
    let current = Catalog::load(&f.catalog.root).unwrap();
    let saved = crate::snapshot_service::read_snapshot(&current)
        .unwrap()
        .unwrap();
    let records: Vec<Event> = saved
        .archives
        .values()
        .flat_map(|text| text.lines().map(|line| serde_json::from_str(line).unwrap()))
        .collect();
    let created: Vec<_> = records
        .iter()
        .filter(|e| e.kind == "baseline.created")
        .collect();
    assert_eq!(created.len(), 1);
    assert_eq!(crate::recovery_outbox::original_of(created[0]), canonical);
    assert_eq!(created[0].data["tags"], json!(["towermd/B0"]));
    assert!(
        !records.iter().any(|e| e.data["commit"] == orphan
            || e.data["tags"]
                .as_array()
                .is_some_and(|tags| tags.iter().any(|tag| tag == "towermd/B100"))),
        "unreachable tags are not recovery facts or reachable-tag diagnostics"
    );
    assert!(records
        .iter()
        .any(|e| e.kind == "alert.raised" && e.data["tags"] == json!(["towermd/B99"])));
    assert_eq!(f.outbox.read_inputs().unwrap().digest(), input.digest());
    drop(session);
    std::fs::remove_dir_all(current.common_dir.join("towermd")).unwrap();
    drop(f.outbox);
    for entry in std::fs::read_dir(f.storage.path()).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    let empty = RecoveryInputs::read(f.storage.path(), &current).unwrap();
    let request = crate::recovery::prepare(
        &current,
        &empty,
        2100,
        10000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    let (_, session) = crate::snapshot_service::SnapshotWriter::default()
        .confirm_rebuild(
            None,
            &current,
            &request,
            "human:fixture",
            10000,
            &empty,
            "h",
            || Ok(2200),
        )
        .unwrap();
    let current = Catalog::load(&current.root).unwrap();
    let saved = crate::snapshot_service::read_snapshot(&current)
        .unwrap()
        .unwrap();
    assert_eq!(
        saved
            .archives
            .values()
            .flat_map(|text| text.lines())
            .filter(|line| serde_json::from_str::<Event>(line).unwrap().kind == "baseline.created")
            .count(),
        1
    );
    assert!(session.lock().unwrap().projection().is_ok());
}

#[test]
fn recovery_facts_deduplicate_actual_two_parent_entry_and_follow_commit_topology() {
    let mut f = MergeFixture::with_shape(false, "consumer");
    seed_snapshot_before_merge(&mut f);
    let tip = f.input.commit.clone();
    let base = f.started.data["input"]["base"].as_str().unwrap();
    let base_tree = git(&f.catalog.root, &["rev-parse", &format!("{base}^{{tree}}")]);
    let between = git(
        &f.catalog.root,
        &[
            "commit-tree",
            &base_tree,
            "-p",
            base,
            "-m",
            "intermediate main fact\n\nTowermd-Future: before entry",
        ],
    );
    let tree = git(&f.catalog.root, &["rev-parse", &format!("{tip}^{{tree}}")]);
    let side_audit = git(
        &f.catalog.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &tip,
            "-m",
            "later side audit\n\nTowermd-Future: after side Task",
        ],
    );
    let entry = git(
        &f.catalog.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &between,
            "-p",
            &side_audit,
            "-m",
            &format!(
                "actual two-parent entry\n\n{}",
                render_task_trailers(&f.input.trailers).unwrap()
            ),
        ],
    );
    git(
        &f.catalog.root,
        &["update-ref", "refs/heads/main", &entry, &tip],
    );
    f.catalog = Catalog::load(&f.catalog.root).unwrap();
    let mut gate_capture: FrozenFinish =
        serde_json::from_value(f.started.data["input"].clone()).unwrap();
    gate_capture.commit = side_audit.clone();
    gate_capture.base = between.clone();
    gate_capture.authority = between.clone();
    terminal_fixture(
        &mut f.hub,
        &mut f.outbox,
        &f.catalog,
        &gate_capture,
        &f.completed,
        None,
    );
    // These tags point before the snapshot prefix, so they are supplemental facts,
    // not candidates discovered in the new-commit traversal above.
    let older = git(&f.catalog.root, &["rev-parse", &format!("{base}^")]);
    git(&f.catalog.root, &["tag", "towermd/B90", &older]);
    git(&f.catalog.root, &["tag", "towermd/B91", base]);
    let inputs = f.outbox.read_inputs().unwrap();
    let history = read_recovery_history(&f.catalog, &inputs).unwrap();
    let expected = prove_merge(&f.catalog, &history, &f.input)
        .unwrap()
        .into_event();
    assert_eq!(expected.data["commit"], entry);
    let direct = prove_merge(
        &f.catalog,
        &history,
        &MergeProofInput {
            commit: entry.clone(),
            trailers: f.input.trailers.clone(),
        },
    )
    .unwrap()
    .into_event();
    assert_eq!(direct, expected);
    let saved = crate::snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    let facts = crate::recovery::recover_git_facts(&f.catalog, &saved, &history).unwrap();
    let merges: Vec<_> = facts
        .iter()
        .filter(|event| event.kind == "merge.done")
        .collect();
    let order = git(
        &f.catalog.root,
        &["rev-list", "--reverse", "--topo-order", &entry],
    );
    let positions: BTreeMap<_, _> = order
        .lines()
        .enumerate()
        .map(|(i, commit)| (commit, i))
        .collect();
    let ranks: Vec<_> = facts
        .iter()
        .map(|event| positions[event.data["commit"].as_str().unwrap()])
        .collect();
    assert!(
        ranks.windows(2).all(|pair| pair[0] <= pair[1]),
        "canonical facts out of actual commit order: {facts:?}"
    );
    assert_eq!(
        merges.len(),
        1,
        "side tip and actual entry must form one canonical fact"
    );
    assert_eq!(*merges[0], expected);
    for tagged in [&older, &base.to_string()] {
        assert!(
            facts
                .iter()
                .any(|event| event.kind == "alert.raised" && event.data["commit"] == *tagged),
            "supplemental reachable tag must join canonical topology: {tagged}"
        );
    }
    let alert = facts
        .iter()
        .find(|event| event.data["commit"] == between && event.kind == "alert.raised")
        .unwrap();
    assert_eq!(
        alert.eid,
        crate::events::derived_eid(&between, "alert.raised").unwrap()
    );
}

fn raw_ordinary_merged_recovery(confirmed: bool) {
    let mut f = MergeFixture::with_shape(false, "consumer");
    seed_snapshot_before_merge(&mut f);
    let base = f.started.data["input"]["base"].as_str().unwrap();
    let tree = git(
        &f.catalog.root,
        &["rev-parse", &format!("{}^{{tree}}", f.input.commit)],
    );
    let plain = git(
        &f.catalog.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            base,
            "-m",
            "ordinary code with no Task proof trailers",
        ],
    );
    git(
        &f.catalog.root,
        &["update-ref", "refs/heads/main", &plain, &f.input.commit],
    );
    f.catalog = Catalog::load(&f.catalog.root).unwrap();
    let mut task = f.claim.data["task"].clone();
    task["state"] = json!("merged");
    let raw = NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "h".into(),
        actor: "process:self-reported".into(),
        kind: "task.state".into(),
        layer: "L4".into(),
        obj: "T".into(),
        data: json!({"task":task,"commit":plain}),
        guards: vec![],
    };
    if confirmed {
        append_fixture(&mut f.hub, &mut f.outbox, raw.clone());
    } else {
        f.outbox.append_pending(&raw).unwrap();
    }
    drop(f.outbox);
    f.outbox = crate::recovery_outbox::RecoveryOutbox::open(
        f.storage.path(),
        crate::recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
    )
    .unwrap();
    let inputs = f.outbox.read_inputs().unwrap();
    let before_refs = git(&f.catalog.root, &["show-ref"]);
    let result = crate::recovery::prepare(
        &f.catalog,
        &inputs,
        1000,
        10000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .and_then(|request| {
        crate::snapshot_service::SnapshotWriter::default().confirm_rebuild(
            None,
            &f.catalog,
            &request,
            "human:fixture",
            10000,
            &inputs,
            "h",
            || Ok(2000),
        )
    });
    match result {
        Ok((_, session)) => {
            let projection = session.lock().unwrap().projection().unwrap().clone();
            assert!(projection.objects.get("Task").and_then(|tasks| tasks.get("T")).is_none_or(|task| task["state"] != "merged"), "ordinary self-report became merged without actual Git proof, confirmed={confirmed}");
            let current = Catalog::load(&f.catalog.root).unwrap();
            let business = crate::project_session::ProjectSession::read_status_shared(
                &session, &current, &inputs, "h", None, 2001,
            )
            .unwrap();
            assert!(!business["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|task| task["state"] == "merged"));
            assert!(!projection.objects.contains_key("Evidence"));
        }
        Err(error) => {
            eprintln!("ordinary merged confirmed={confirmed} rejected: {error}");
            assert!(
                error.contains("raw task event cannot certify merged"),
                "unexpected fixture failure: {error}"
            );
            assert_eq!(git(&f.catalog.root, &["show-ref"]), before_refs);
        }
    }
}
#[test]
fn ordinary_pending_merged_self_report_cannot_complete_recovery() {
    raw_ordinary_merged_recovery(false);
}
#[test]
fn ordinary_confirmed_merged_self_report_cannot_complete_recovery() {
    raw_ordinary_merged_recovery(true);
}

fn matrix_dependent() -> NewEvent {
    NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: "h".into(),
        actor: "process:towermd-cli".into(),
        kind: "task.state".into(),
        layer: "L4".into(),
        obj: "D".into(),
        guards: vec![],
        data: json!({"task":{"type":"Task","id":"D","title":"dependent",
            "module":"M","touches":[],"allowed_paths":["code.txt"],
            "depends_on":["T"],"acceptance":["AC"],"decisions":[],"state":"proposed"}}),
    }
}

// Reissue selected synthetic producer bodies through a fresh real Hub. There are
// no old snapshots yet. Missing facts are never appended to this source Hub.
// Do not use rewrite_receipt or delete facts after they have entered a checkpoint.
fn matrix_reissue_history(
    f: &mut MergeFixture,
    fault: &str,
    audit_confirmed: Option<bool>,
) -> (u64, Option<Event>, Option<Event>) {
    use crate::hub_protocol::*;
    let storage = tempfile::tempdir().unwrap();
    let hubdir = tempfile::tempdir().unwrap();
    let mut outbox = crate::recovery_outbox::RecoveryOutbox::open(
        storage.path(),
        crate::recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
    )
    .unwrap();
    let mut hub = crate::hub::Hub::open(hubdir.path()).unwrap();
    let module = hub
        .lease_acquire(
            AcquireLease {
                host: "h".into(),
                names: vec!["module:M".into()],
                holder: "R".into(),
                ttl_ms: 100000,
            },
            100,
        )
        .unwrap();
    let fences: Vec<_> = module.leases.iter().map(|l| l.fence()).collect();
    assert_eq!(fences, f.claim.guards); // low-risk, single-module fixture only
    hub.lease_acquire(
        AcquireLease {
            host: "h".into(),
            names: vec!["main".into()],
            holder: "original-writer".into(),
            ttl_ms: 100000,
        },
        100,
    )
    .unwrap();

    let mut audit = None;
    if let Some(confirmed) = audit_confirmed {
        let original = NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "h".into(),
            actor: "process:contract-audit".into(),
            kind: "alert.raised".into(),
            layer: "L4".into(),
            obj: "audit".into(),
            guards: vec![],
            data: json!({"reason":"contract audit","unknown":{"keep":[3,2,1]}}),
        };
        let actual = if confirmed {
            append_fixture(&mut hub, &mut outbox, original)
        } else {
            // Accepted and archived by the real source Hub; local outbox still pending.
            let AppendReceipt::Event(actual) = hub.append(original.clone(), 103).unwrap() else {
                panic!()
            };
            outbox.append_pending(&original).unwrap();
            *actual
        };
        assert!(!actual.stale);
        audit = Some(actual);
    }
    let dependent = append_fixture(&mut hub, &mut outbox, matrix_dependent());
    let prefix_through = dependent.seq;
    let templates = [
        f.claim.clone(),
        f.started.clone(),
        f.completed.clone(),
        f.integration_gate.clone(),
    ];
    for template in templates {
        let omit = match fault {
            "isolated-completed" => {
                matches!(template.kind.as_str(), "task.claimed" | "finish.started")
            }
            "isolated-passed-file" => true,
            "missing-freeze" => template.kind == "finish.started",
            "missing-pass" => template.kind == "finish.completed",
            "missing-terminal" => template.kind == "evidence.recorded",
            "original-stale-pass" | "valid" => false,
            _ => panic!("unknown matrix fault {fault}"),
        };
        if omit {
            continue;
        }
        if fault == "original-stale-pass" && template.kind == "finish.completed" {
            hub.lease_release(
                ReleaseLease {
                    host: "h".into(),
                    fences: fences.clone(),
                },
                103,
            )
            .unwrap();
        }
        let actual = append_fixture(
            &mut hub,
            &mut outbox,
            crate::recovery_outbox::original_of(&template),
        );
        if template.kind == "finish.completed" && fault == "original-stale-pass" {
            assert!(
                actual.stale,
                "real source Hub must produce the stale receipt"
            );
        }
        match template.kind.as_str() {
            "task.claimed" => f.claim = actual,
            "finish.started" => f.started = actual,
            "finish.completed" => f.completed = actual,
            "evidence.recorded" => f.integration_gate = actual,
            _ => unreachable!(),
        }
    }
    // Use a real dedicated-protocol renewal as the legacy envelope's original body.
    // It is outside the source checkpoint; legacy reader must discard this row.
    let renewal = if audit_confirmed.is_some() {
        Some(
            hub.lease_renew(
                RenewLease {
                    host: "h".into(),
                    fences,
                    ttl_ms: 100000,
                },
                104,
            )
            .unwrap()
            .event,
        )
    } else {
        None
    };
    // Assignment drops each old lock before its old temporary directory is removed.
    f.outbox = outbox;
    f.storage = storage;
    f.hub = hub;
    f._hubdir = hubdir;
    (prefix_through, audit, renewal)
}

// Generalization of frozen seed_snapshot_before_merge(:1820): explicit source
// prefix rather than f.claim.seq, so isolated-pass sources retain actual D.
fn matrix_seed_prefix(f: &mut MergeFixture, through: u64) {
    use crate::hub_protocol::HubProtocol;
    let frozen: FrozenFinish = serde_json::from_value(f.started.data["input"].clone()).unwrap();
    git(
        &f.catalog.root,
        &[
            "update-ref",
            "refs/heads/main",
            &frozen.base,
            &f.input.commit,
        ],
    );
    let catalog = Catalog::load(&f.catalog.root).unwrap();
    let prefix: Vec<_> = f
        .hub
        .subscribe(0, 104)
        .unwrap()
        .events
        .into_iter()
        .take_while(|e| e.seq <= through)
        .collect();
    assert_eq!(prefix.last().unwrap().seq, through);
    let snapshot =
        crate::snapshot::Snapshot::from_events(catalog.config.project_id(), &prefix, through, 2)
            .unwrap();
    assert_eq!(
        snapshot.projection.objects["Task"]["D"]["state"],
        "proposed"
    );
    assert!(!snapshot.projection.objects["Task"].contains_key("T"));
    let mut archives = BTreeMap::<String, String>::new();
    for event in prefix {
        archives
            .entry(event.t[..7].into())
            .or_default()
            .push_str(&format!("{}\n", serde_json::to_string(&event).unwrap()));
    }
    let write = crate::snapshot_git::WriteRequest {
        base: catalog.revision.clone(),
        docs_dir: catalog.config.docs_dir().into(),
        state_json: serde_json::to_string(&snapshot).unwrap(),
        archives,
        through_seq: snapshot.through_seq,
        writer_epoch: snapshot.writer_epoch,
    };
    assert!(matches!(
        crate::snapshot_git::write_snapshot(&catalog.root, &write, || Ok(())).unwrap(),
        crate::snapshot_git::WriteOutcome::Written { .. }
    ));
    git(
        &catalog.root,
        &[
            "update-ref",
            "refs/heads/main",
            &f.input.commit,
            &frozen.base,
        ],
    );
    f.catalog = Catalog::load(&catalog.root).unwrap();
}

fn matrix_records(catalog: &Catalog) -> Vec<Event> {
    let saved = crate::snapshot_service::read_snapshot(catalog)
        .unwrap()
        .unwrap();
    let records: Vec<Event> = saved
        .archives
        .values()
        .flat_map(|text| {
            text.lines()
                .map(|line| crate::hub_legacy::decode_archive_line(line).unwrap())
        })
        .collect();
    saved
        .snapshot
        .validate_archive(catalog.config.project_id(), &records)
        .unwrap();
    records
}
fn matrix_confirm(
    catalog: &Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    now: u64,
) -> std::sync::Arc<std::sync::Mutex<crate::project_session::ProjectSession>> {
    let request = crate::recovery::prepare(
        catalog,
        inputs,
        now - 100,
        10000,
        &uuid::Uuid::now_v7().to_string(),
    )
    .unwrap();
    crate::snapshot_service::SnapshotWriter::default()
        .confirm_rebuild(
            None,
            catalog,
            &request,
            "human:fixture",
            10000,
            inputs,
            "h",
            || Ok(now),
        )
        .unwrap()
        .1
}
fn matrix_claim_dependent(
    session: &std::sync::Arc<std::sync::Mutex<crate::project_session::ProjectSession>>,
    root: &std::path::Path,
    storage: &std::path::Path,
    succeeds: bool,
    now: u64,
) {
    git(root, &["switch", "-qc", "t/D"]);
    let catalog = Catalog::load(root).unwrap();
    let inputs = crate::recovery_outbox::RecoveryInputs::read(storage, &catalog).unwrap();
    let before = session.lock().unwrap().applied_seq();
    let result = crate::project_session::ProjectSession::execute_shared(
        session,
        &catalog,
        &inputs,
        "h",
        crate::tasks::TaskCommand::Claim {
            id: "D".into(),
            harness: None,
            harness_version: None,
        },
        None,
        now,
    );
    let events = session.lock().unwrap().events(before);
    let ready = events
        .iter()
        .position(|e| e.kind == "task.state" && e.obj == "D" && e.data["task"]["state"] == "ready");
    let claimed = events
        .iter()
        .position(|e| e.kind == "task.claimed" && e.obj == "D");
    if succeeds {
        result.unwrap();
        assert!(ready.unwrap() < claimed.unwrap());
        assert_eq!(
            session.lock().unwrap().projection().unwrap().objects["Task"]["D"]["state"],
            "running"
        );
    } else {
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("dependencies are missing or not merged"),
            "{error}"
        );
        assert!(ready.is_none() && claimed.is_none());
        assert_eq!(
            session.lock().unwrap().projection().unwrap().objects["Task"]["D"]["state"],
            "proposed"
        );
    }
}

#[test]
fn matrix_incomplete_or_original_stale_finish_cannot_merge_after_product_recovery() {
    for fault in [
        "isolated-completed",
        "isolated-passed-file",
        "missing-freeze",
        "missing-pass",
        "missing-terminal",
        "original-stale-pass",
    ] {
        // REQUIRED tiny fixture amendment described in README: "consumer-passed-file"
        // also creates AC and an isolated passed Evidence file in original base.
        let shape = if fault == "isolated-passed-file" {
            "consumer-passed-file"
        } else {
            "consumer"
        };
        let mut f = MergeFixture::with_shape(false, shape);
        let (through, _, _) = matrix_reissue_history(&mut f, fault, None);
        matrix_seed_prefix(&mut f, through);
        drop(f.hub);
        drop(f._hubdir);
        drop(f.outbox); // actual source Hub/outbox handles lost
        let reopened = crate::recovery_outbox::RecoveryOutbox::open(
            f.storage.path(),
            crate::recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
        )
        .unwrap();
        let inputs = reopened.read_inputs().unwrap();
        if fault == "original-stale-pass" {
            assert!(inputs
                .confirmed()
                .iter()
                .any(|e| e.eid == f.completed.eid && e.stale));
        }
        let session = matrix_confirm(&f.catalog, &inputs, 2000);
        let projection = session.lock().unwrap().projection().unwrap().clone();
        // Original guarded envelopes replay stale. Neither an active Run nor E is revived.
        assert!(
            projection
                .objects
                .get("Task")
                .and_then(|m| m.get("T"))
                .is_none(),
            "{fault}"
        );
        assert!(
            projection
                .objects
                .get("Run")
                .and_then(|m| m.get("R"))
                .is_none(),
            "{fault}"
        );
        assert!(
            projection
                .objects
                .get("Evidence")
                .and_then(|m| m.get("E"))
                .is_none(),
            "{fault}"
        );
        assert_eq!(
            projection.objects["Task"]["D"]["state"], "proposed",
            "{fault}"
        );
        let current = Catalog::load(&f.catalog.root).unwrap();
        let records = matrix_records(&current);
        assert!(!records.iter().any(|e| e.kind == "merge.done"), "{fault}");
        assert!(records.iter().any(|e| e.kind == "gate.failed"), "{fault}");
        assert!(records.iter().any(|e| e.kind == "hold.opened"), "{fault}");
        matrix_claim_dependent(&session, &current.root, f.storage.path(), false, 2100);
    }
}

#[test]
fn matrix_persisted_reopen_archive_dedup_legacy_renew_and_guardless_audit() {
    use std::io::Write;
    for confirmed in [false, true] {
        let mut f = MergeFixture::with_shape(false, "consumer");
        let (through, audit, renewal) = matrix_reissue_history(&mut f, "valid", Some(confirmed));
        let audit = audit.unwrap();
        let renewal = renewal.unwrap();
        matrix_seed_prefix(&mut f, through);
        // A second guardless audit is NOT in the source archive: exercise actual
        // outbox replay retention as well as the archived audit's deduplication.
        let ordinary = NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "h".into(),
            actor: "process:contract-audit".into(),
            kind: "alert.raised".into(),
            layer: "L4".into(),
            obj: "ordinary-audit".into(),
            guards: vec![],
            data: json!({"reason":"ordinary audit after source prefix","keep":[3,2,1]}),
        };
        if confirmed {
            append_fixture(&mut f.hub, &mut f.outbox, ordinary.clone());
        } else {
            f.outbox.append_pending(&ordinary).unwrap();
        }
        let row_path = std::fs::read_dir(f.storage.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().and_then(|s| s.to_str()) == Some("jsonl"))
            .unwrap();
        let text = std::fs::read_to_string(&row_path).unwrap();
        let mut legacy: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        legacy["state"] = json!("pending");
        legacy.as_object_mut().unwrap().remove("actual");
        legacy["original"] =
            serde_json::to_value(crate::recovery_outbox::original_of(&renewal)).unwrap();
        // Legacy-only fixture: current API correctly refuses lease.* business outbox.
        drop(f.hub);
        drop(f._hubdir);
        drop(f.outbox);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&row_path)
            .unwrap();
        writeln!(file, "{}", serde_json::to_string(&legacy).unwrap()).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let reopened = crate::recovery_outbox::RecoveryOutbox::open(
            f.storage.path(),
            crate::recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
        )
        .unwrap();
        let inputs = reopened.read_inputs().unwrap();
        assert_eq!(
            inputs.confirmed().iter().any(|e| e.eid == audit.eid),
            confirmed
        );
        assert_eq!(
            inputs.pending().iter().any(|e| e.eid == audit.eid),
            !confirmed
        );
        assert!(!inputs.pending().iter().any(|e| e.eid == renewal.eid));
        assert!(!inputs.confirmed().iter().any(|e| e.eid == renewal.eid));
        let digest = inputs.digest().to_owned();
        let session = matrix_confirm(&f.catalog, &inputs, 2000);
        let current = Catalog::load(&f.catalog.root).unwrap();
        let projection = session.lock().unwrap().projection().unwrap().clone();
        assert_eq!(projection.objects["Task"]["T"]["state"], "merged");
        assert_eq!(projection.objects["Run"]["R"]["state"], "ended");
        assert_eq!(projection.objects["Evidence"]["E"]["id"], "E");
        assert_eq!(projection.objects["Task"]["D"]["state"], "proposed");
        let records = matrix_records(&current);
        let copies: Vec<_> = records.iter().filter(|e| e.eid == audit.eid).collect();
        assert_eq!(copies, vec![&audit]); // exact original seq/t/body/eid, not new replay coordinates
        assert!(audit.guards.is_empty() && !audit.stale);
        let ordinary_copies: Vec<_> = records.iter().filter(|e| e.eid == ordinary.eid).collect();
        assert_eq!(ordinary_copies.len(), 1);
        let ordinary_replay = (*ordinary_copies[0]).clone();
        assert_eq!(
            crate::recovery_outbox::original_of(&ordinary_replay),
            ordinary
        );
        assert!(ordinary_replay.guards.is_empty() && !ordinary_replay.stale);
        assert!(!records.iter().any(|e| e.eid == renewal.eid));
        assert!(records
            .iter()
            .any(|e| e.eid == f.claim.eid && e.stale && e.guards == f.claim.guards));
        assert_eq!(reopened.read_inputs().unwrap().digest(), digest); // no second confirmed
        drop(session);
        std::fs::remove_dir_all(current.common_dir.join("towermd")).unwrap();
        drop(reopened);
        let reopened = crate::recovery_outbox::RecoveryOutbox::open(
            f.storage.path(),
            crate::recovery_outbox::RecoveryContext::from_catalog(&current).unwrap(),
        )
        .unwrap();
        let again = reopened.read_inputs().unwrap();
        let restored = matrix_confirm(&current, &again, 2200);
        let latest = Catalog::load(&current.root).unwrap();
        let twice = matrix_records(&latest);
        assert_eq!(
            twice
                .iter()
                .filter(|e| e.eid == audit.eid)
                .collect::<Vec<_>>(),
            vec![&audit]
        );
        assert_eq!(
            twice
                .iter()
                .filter(|e| e.eid == ordinary.eid)
                .collect::<Vec<_>>(),
            vec![&ordinary_replay]
        );
        assert!(!twice.iter().any(|e| e.eid == renewal.eid));
        assert_eq!(twice.iter().filter(|e| e.kind == "merge.done").count(), 1);
        let p = restored.lock().unwrap().projection().unwrap().clone();
        assert_eq!(p.objects["Task"]["T"]["state"], "merged");
        assert_eq!(p.objects["Run"]["R"]["state"], "ended");
        assert_eq!(p.objects["Evidence"]["E"]["id"], "E");
        assert_eq!(p.objects["Task"]["D"]["state"], "proposed");
        assert_eq!(reopened.read_inputs().unwrap().digest(), digest);
        matrix_claim_dependent(&restored, &latest.root, f.storage.path(), true, 2300);
    }
}

#[test]
fn matrix_snapshot_canvas_only_differences_restore_merge_and_unblock_real_g0() {
    let mut f = MergeFixture::with_shape(false, "consumer");
    let (through, _, _) = matrix_reissue_history(&mut f, "valid", None);
    matrix_seed_prefix(&mut f, through);
    let frozen: FrozenFinish = serde_json::from_value(f.started.data["input"].clone()).unwrap();
    let saved = crate::snapshot_service::read_snapshot(&f.catalog)
        .unwrap()
        .unwrap();
    // Frozen :1064 state-only rebase pattern, plus a genuine canvas-only main edge.
    git(
        &f.catalog.root,
        &["switch", "-qc", "matrix-state-base", &saved.commit],
    );
    std::fs::write(
        f.catalog.root.join("docs/tower/tower.canvas"),
        "{\"nodes\":[],\"edges\":[]}\n",
    )
    .unwrap();
    git(&f.catalog.root, &["add", "docs/tower/tower.canvas"]);
    git(
        &f.catalog.root,
        &["commit", "-qm", "contract canvas-only main advance"],
    );
    let base = git(&f.catalog.root, &["rev-parse", "HEAD"]);
    git(
        &f.catalog.root,
        &["update-ref", "refs/heads/main", &base, &f.input.commit],
    );
    git(
        &f.catalog.root,
        &["switch", "-qc", "matrix-rebased-task", &base],
    );
    std::fs::write(f.catalog.root.join("code.txt"), "task\n").unwrap();
    git(&f.catalog.root, &["add", "code.txt"]);
    git(
        &f.catalog.root,
        &[
            "commit",
            "-qm",
            &format!(
                "contract state/canvas-only rebase\n\n{}",
                render_task_trailers(&f.input.trailers).unwrap()
            ),
        ],
    );
    let task = git(&f.catalog.root, &["rev-parse", "HEAD"]);
    let tree = git(&f.catalog.root, &["rev-parse", "HEAD^{tree}"]);
    assert_ne!(tree, frozen.tree);
    let differences = git(
        &f.catalog.root,
        &["diff", "--name-only", &frozen.tree, &tree],
    );
    assert!(differences
        .lines()
        .all(|p| p.starts_with("docs/tower/state/") || p == "docs/tower/tower.canvas"));
    assert!(differences
        .lines()
        .any(|p| p.starts_with("docs/tower/state/")));
    assert!(differences.lines().any(|p| p == "docs/tower/tower.canvas"));
    let capture = FrozenFinish {
        commit: task.clone(),
        tree: tree.clone(),
        base: base.clone(),
        authority: base.clone(),
        ..frozen.clone()
    };
    let gate = terminal_fixture(
        &mut f.hub,
        &mut f.outbox,
        &Catalog::load(&f.catalog.root).unwrap(),
        &capture,
        &f.completed,
        None,
    );
    git(
        &f.catalog.root,
        &["update-ref", "refs/heads/main", &task, &base],
    );
    git(&f.catalog.root, &["switch", "-q", "main"]);
    f.input.commit = task;
    f.catalog = Catalog::load(&f.catalog.root).unwrap();
    drop(f.hub);
    drop(f._hubdir);
    drop(f.outbox);
    let reopened = crate::recovery_outbox::RecoveryOutbox::open(
        f.storage.path(),
        crate::recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
    )
    .unwrap();
    let inputs = reopened.read_inputs().unwrap();
    let session = matrix_confirm(&f.catalog, &inputs, 2000);
    let p = session.lock().unwrap().projection().unwrap().clone();
    assert_eq!(p.objects["Task"]["T"]["state"], "merged");
    assert_eq!(p.objects["Run"]["R"]["state"], "ended");
    assert_eq!(p.objects["Evidence"]["E"]["tree"], frozen.tree);
    assert_eq!(p.objects["Task"]["D"]["state"], "proposed");
    let current = Catalog::load(&f.catalog.root).unwrap();
    let records = matrix_records(&current);
    let merges: Vec<_> = records.iter().filter(|e| e.kind == "merge.done").collect();
    assert_eq!(merges.len(), 1);
    assert_eq!(merges[0].data["commit"], f.input.commit);
    assert_eq!(merges[0].data["tree"], tree);
    assert_eq!(
        merges[0].data["proof"]["completed"],
        serde_json::to_value(&f.completed).unwrap()
    );
    assert_eq!(
        merges[0].data["proof"]["integration_gate"],
        serde_json::to_value(&gate).unwrap()
    );
    assert!(records
        .iter()
        .any(|e| e.eid == f.claim.eid && e.stale && e.guards == f.claim.guards));
    assert_eq!(reopened.read_inputs().unwrap().digest(), inputs.digest());
    matrix_claim_dependent(&session, &current.root, f.storage.path(), true, 2100);
}

#[test]
fn matrix_persisted_raw_merge_done_cannot_install_business_completion() {
    for confirmed in [false, true] {
        let mut f = MergeFixture::with_shape(false, "consumer");
        let (through, _, _) = matrix_reissue_history(&mut f, "missing-pass", None);
        matrix_seed_prefix(&mut f, through);
        // Same real Task-trailer Git commit, but no original completed receipt.
        // verified=true and embedded passed Evidence are self-report, not canonical proof.
        let mut task = f.claim.data["task"].clone();
        task["state"] = json!("merged");
        let mut run = f.claim.data["run"].clone();
        run["state"] = json!("ended");
        let self_report = NewEvent {
            eid: uuid::Uuid::now_v7().to_string(),
            host: "h".into(),
            actor: "process:towermd-integrator".into(),
            kind: "merge.done".into(),
            layer: "L4".into(),
            obj: "T".into(),
            guards: vec![],
            data: json!({"verified":true,"commit":f.input.commit,"task":task,"run":run,
                "evidence":f.completed.data["evidence"]}),
        };
        if confirmed {
            append_fixture(&mut f.hub, &mut f.outbox, self_report);
        } else {
            f.outbox.append_pending(&self_report).unwrap();
        }
        let before = crate::snapshot_service::read_snapshot(&f.catalog)
            .unwrap()
            .unwrap();
        let private_before = crate::snapshot_git::private_revision(&f.catalog.root).unwrap();
        let main_before = f.catalog.revision.clone();
        drop(f.hub);
        drop(f._hubdir);
        drop(f.outbox);
        let reopened = crate::recovery_outbox::RecoveryOutbox::open(
            f.storage.path(),
            crate::recovery_outbox::RecoveryContext::from_catalog(&f.catalog).unwrap(),
        )
        .unwrap();
        let inputs = reopened.read_inputs().unwrap();
        let prepared = crate::recovery::prepare(
            &f.catalog,
            &inputs,
            1000,
            10000,
            &uuid::Uuid::now_v7().to_string(),
        );
        if confirmed {
            // Frozen read_recovery_history validates every confirmed canonical kind.
            let error = prepared.unwrap_err();
            assert!(error.contains("invalid canonical merge proof"), "{error}");
        } else {
            let request = prepared.unwrap();
            let result = crate::snapshot_service::SnapshotWriter::default().confirm_rebuild(
                None,
                &f.catalog,
                &request,
                "human:fixture",
                10000,
                &inputs,
                "h",
                || Ok(2000),
            );
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("pending merge must be re-proved before installation"),
            };
            assert!(
                error.contains("merge requires unique original freeze/pass chain"),
                "{error}"
            );
        }
        let current = Catalog::load(&f.catalog.root).unwrap();
        assert_eq!(current.revision, main_before);
        assert_eq!(
            crate::snapshot_git::private_revision(&current.root).unwrap(),
            private_before
        );
        let after = crate::snapshot_service::read_snapshot(&current)
            .unwrap()
            .unwrap();
        assert_eq!(after.commit, before.commit);
        assert_eq!(after.snapshot.projection, before.snapshot.projection);
        let p = &after.snapshot.projection;
        assert!(p.objects.get("Task").and_then(|m| m.get("T")).is_none());
        assert!(p.objects.get("Run").and_then(|m| m.get("R")).is_none());
        assert!(p.objects.get("Evidence").and_then(|m| m.get("E")).is_none());
        assert_eq!(p.objects["Task"]["D"]["state"], "proposed");
        assert!(!matrix_records(&current)
            .iter()
            .any(|e| e.kind == "merge.done"));
        // There is intentionally no active ProjectSession on rejection. Inspect only
        // an already-created raw target to prove staging never installed into it.
        if current.common_dir.join("towermd").exists() {
            use crate::hub_protocol::HubProtocol;
            let mut target = crate::hub::Hub::open(&current.common_dir.join("towermd")).unwrap();
            let batch = target.subscribe(0, 2000).unwrap();
            assert_eq!(batch.head.contiguous_seq, 0);
            assert!(batch.events.is_empty() && batch.head.leases.is_empty());
        }
    }
}
