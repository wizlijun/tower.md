use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use towermd_core::hub::{AppendReceipt,Event,Hub,NewEvent};
use towermd_core::hub_protocol::HubProtocol;
use towermd_core::question_model::*;

struct Events { _dir:TempDir,hub:Hub,now:u64 }
impl Events {
    fn new()->Self {
        let dir=TempDir::new().unwrap();
        let hub=Hub::open(dir.path()).unwrap();
        Self{_dir:dir,hub,now:1000}
    }
    fn event(&mut self,kind:&str,actor:&str,obj:&str,data:QuestionEvent)->Event {
        self.now+=1;
        match self.hub.append(NewEvent {
            eid:uuid::Uuid::now_v7().to_string(),host:"local".into(),actor:actor.into(),
            kind:kind.into(),layer:"L5".into(),obj:obj.into(),
            data:serde_json::to_value(data).unwrap(),guards:vec![],
        },self.now).unwrap() {
            AppendReceipt::Event(event)=>*event,
            _=>panic!("new event required"),
        }
    }
}
fn binding(run:&str,module:&str)->RunContext {
    RunContext{run:run.into(),task:format!("T-{run}"),scope:module.into()}
}
fn question(id:&str,run:&str,module:&str,text:&str)->Question {
    prepare_question(QuestionDraft {
        id:id.into(),text:text.into(),options:vec!["A".into(),"B".into()],
        recommended:Some(0),context:vec!["src/main.rs".into()],mode:WaitMode::Live,
    },binding(run,module),vec![],1000).unwrap()
}
fn open(state:&mut QuestionProjection,events:&mut Events,question:Question) {
    let event=events.event("hold.opened","process:towermd-cli",&question.draft.id.clone(),
        QuestionEvent::Opened{question});
    state.apply(&event).unwrap();
}
fn answer(state:&mut QuestionProjection,events:&mut Events,hold:&str,text:&str,remember_as:RememberAs)->Event {
    let event=events.event("hold.answered","human:local",hold,QuestionEvent::Answered {
        answer:AnswerInput{hold:hold.into(),text:text.into(),remember_as,targets:vec![],assumption:None},
    });
    state.apply(&event).unwrap();
    event
}

#[test]
fn normalization_is_nfkc_whitespace_ascii_case_and_only_trailing_punctuation() {
    assert_eq!(normalize_question(" \u{3000}ＦＯＯ\t  BAR！？。 "), "foo bar");
    assert_eq!(normalize_question("ﬃ K e\u{301} ? !"),"ffi k é");
    assert_eq!(normalize_question("Ä A"),"Ä a");
    assert_eq!(normalize_question("C++"),"c++");
    assert_eq!(normalize_question("cost $"),"cost $");
    assert_eq!(normalize_question("Why? now ؟"),"why? now");
    assert_eq!(normalize_question("选择 😀"),"选择 😀");
    assert_eq!(normalize_question("new punctuation\u{11f43}"),"new punctuation");
    assert_eq!(fingerprint("A-test","Output ＪＳＯＮ？").unwrap(),fingerprint("A-test"," output json ").unwrap());
    assert_ne!(fingerprint("A-test","json?").unwrap(),fingerprint("A-other","json?").unwrap());
    assert_ne!(fingerprint("A-test","a? b").unwrap(),fingerprint("A-test","a b").unwrap());
    assert!(fingerprint("A-test","！？").is_err());
}

#[test]
fn wait_is_bounded_and_deadline_never_resets_between_calls() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    open(&mut state,&mut events,question("H-1","R-1","A-a","格式？"));
    assert!(matches!(state.wait_plan("H-1",1000,Some(u64::MAX)).unwrap(),WaitPlan::Pending{wait_ms:90_000,..}));
    assert!(matches!(state.wait_plan("H-1",600_999,None).unwrap(),WaitPlan::Pending{wait_ms:1,..}));
    assert!(matches!(state.wait_plan("H-1",601_000,None).unwrap(),WaitPlan::Deferred{proposal:None,..}));
    assert_eq!(state.holds["H-1"].deadline,601_000);
    let mut parked=question("H-2","R-2","A-a","另一问题？");parked.draft.mode=WaitMode::Parked;
    open(&mut state,&mut events,parked);
    assert!(matches!(state.wait_plan("H-2",1000,None).unwrap(),WaitPlan::Deferred{..}));
    answer(&mut state,&mut events,"H-2","纯文本",RememberAs::Temporary);
    assert!(matches!(state.wait_plan("H-2",999_000,None).unwrap(),WaitPlan::Answered{..}));
}

#[test]
fn temporary_answers_are_private_to_the_origin_run_and_never_deduplicate() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    open(&mut state,&mut events,question("H-1","R-1","A-a","格式？"));
    answer(&mut state,&mut events,"H-1","JSON",RememberAs::Temporary);
    assert_eq!(state.pending_messages("R-1").len(),1);
    assert!(state.pending_messages("R-2").is_empty());
    let catalog=DecisionCatalog::from_projection(&state);
    assert_eq!(catalog.lookup("A-a","格式?").unwrap(),DecisionMatch::Missing);
    assert!(catalog.reuse_event(&question("H-2","R-2","A-a","格式?")).unwrap().is_none());
}

#[test]
fn relayed_human_answers_remain_process_authored_temporary_and_replayable() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    let opened=events.event("hold.opened","process:towermd-cli","H-1",
        QuestionEvent::Opened{question:question("H-1","R-1","A-a","格式？")});
    state.apply(&opened).unwrap();
    let relayed=events.event("hold.answered","process:towermd-cli","H-1",QuestionEvent::AnswerRelayed{
        hold:"H-1".into(),text:"JSON，仅本次".into(),relayed_from:"human".into(),
        self_reported:Some(SelfReported{name:"test-harness".into(),version:"1".into(),..Default::default()}),
    });
    state.apply(&relayed).unwrap();state.apply(&relayed).unwrap();
    assert_eq!(relayed.data["relayed_from"],"human");
    assert_eq!(relayed.data["self_reported"]["name"],"test-harness");
    let accepted=&state.answers["H-1"];
    assert_eq!(accepted.actor,"process:towermd-cli");
    assert_eq!(accepted.input.remember_as,RememberAs::Temporary);
    assert!(accepted.proposal.is_none()&&accepted.decision.is_none()&&accepted.publication.is_none());
    assert!(matches!(state.wait_plan("H-1",1100,None).unwrap(),WaitPlan::Answered{..}));
    assert!(state.decisions.is_empty()&&state.rules.is_empty()&&state.proposals.is_empty());
    assert_eq!(DecisionCatalog::from_projection(&state).lookup("A-a","格式？").unwrap(),DecisionMatch::Missing);
    assert_eq!(state.pending_messages("R-1").len(),1);
    assert!(state.pending_messages("R-other").is_empty());
    let queued=state.pending_messages("R-1")[0].message.clone();
    let delivered=events.event("prompt.delivered","process:towermd-cli","H-1",QuestionEvent::Delivered{
        delivery:Delivery{run:"R-1".into(),message:queued},
    });
    state.apply(&delivered).unwrap();assert!(state.pending_messages("R-1").is_empty());
    let mut replay=QuestionProjection::default();
    for event in [opened,relayed,delivered] {replay.apply(&event).unwrap();}
    assert_eq!(state,replay);
}

#[test]
fn relays_cannot_impersonate_humans_register_rules_or_resolve_slow_questions() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    open(&mut state,&mut events,question("H-1","R-1","A-a","格式？"));
    let action=QuestionEvent::AnswerRelayed{hold:"H-1".into(),text:"JSON".into(),relayed_from:"human".into(),self_reported:None};
    for actor in ["human:local","test-harness/1"] {
        let event=events.event("hold.answered",actor,"H-1",action.clone());
        assert!(state.apply(&event).is_err());
    }
    for key in ["remember_as","targets","actor"] {
        let mut data=serde_json::to_value(&action).unwrap();data[key]=serde_json::json!("forged");
        assert!(serde_json::from_value::<QuestionEvent>(data).is_err(),"{key}");
    }
    let invalid=events.event("hold.answered","process:towermd-cli","H-1",QuestionEvent::AnswerRelayed{
        hold:"H-1".into(),text:"JSON".into(),relayed_from:"agent".into(),self_reported:None,
    });
    assert!(state.apply(&invalid).unwrap_err().contains("human"));
    for layer in ["L0","L1","L2"] {
        let id=format!("H-{layer}");
        let mut slow_question=question(&id,"R-1","A-a","修改结构？");
        slow_question.targets=vec![ChangeTarget{id:"A-a".into(),layer:layer.into()}];
        open(&mut state,&mut events,slow_question);
        let before=state.clone();
        let event=events.event("hold.answered","process:towermd-cli",&id,QuestionEvent::AnswerRelayed{
            hold:id.clone(),text:"批准".into(),relayed_from:"human".into(),self_reported:None,
        });
        assert!(state.apply(&event).unwrap_err().contains("App"));
        assert_eq!(state,before);
    }
    assert!(state.answers.is_empty()&&state.decisions.is_empty()&&state.rules.is_empty()&&state.proposals.is_empty());
}

#[test]
fn only_remembered_human_decisions_enable_exact_same_module_automatic_answers() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    open(&mut state,&mut events,question("H-1","R-1","A-a","Output ＪＳＯＮ？"));
    let original=answer(&mut state,&mut events,"H-1","yes",RememberAs::ModuleDecision{id:"D-1".into(),supersedes:vec![],review_on:None});
    let catalog=DecisionCatalog::from_projection(&state);
    assert_eq!(state.decisions["D-1"].source,DecisionSource::AcceptedEvent{eid:original.eid});
    let next=question("H-2","R-2","A-a"," output json? ");
    let reuse=catalog.reuse_event(&next).unwrap().unwrap();
    let receipt=events.event("hold.decision_reused","process:towermd-dedupe","H-2",reuse);
    state.apply(&receipt).unwrap();
    assert_eq!(state.answers["H-2"].actor,"process:towermd-dedupe");
    assert_eq!(state.answers["H-2"].decision.as_ref().unwrap().id,"D-1");
    assert_eq!(state.pending_messages("R-2")[0].text,"yes");
    assert!(catalog.reuse_event(&question("H-3","R-3","A-b","output json?")).unwrap().is_none());
    assert!(catalog.reuse_event(&question("H-4","R-4","A-a","output yaml?")).unwrap().is_none());
}

#[test]
fn conflicting_decisions_do_not_pick_an_arbitrary_answer_and_supersedes_is_explicit() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    for (i,text) in [(1,"JSON"),(2,"text")] {
        let hold=format!("H-{i}");
        open(&mut state,&mut events,question(&hold,&format!("R-{i}"),"A-a","格式？"));
        answer(&mut state,&mut events,&hold,text,RememberAs::ModuleDecision{id:format!("D-{i}"),supersedes:vec![],review_on:None});
    }
    let catalog=DecisionCatalog::from_projection(&state);
    assert!(matches!(catalog.lookup("A-a","格式?").unwrap(),DecisionMatch::Conflict(ids)if ids.len()==2));
    assert!(catalog.reuse_event(&question("H-3","R-3","A-a","格式?")).unwrap().is_none());
    open(&mut state,&mut events,question("H-3","R-3","A-a","格式?"));
    answer(&mut state,&mut events,"H-3","JSON",RememberAs::ModuleDecision{
        id:"D-3".into(),supersedes:vec!["D-1".into(),"D-2".into()],review_on:None,
    });
    assert!(matches!(DecisionCatalog::from_projection(&state).lookup("A-a","格式?").unwrap(),
        DecisionMatch::Exact(decision)if decision.id=="D-3"));
}

#[test]
fn slow_answers_become_only_proposals_even_when_person_selects_module_decision() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    let mut hold=question("H-1","R-1","A-a","修改模块边界？");
    hold.targets=vec![ChangeTarget{id:"A-a".into(),layer:"L1".into()}];
    open(&mut state,&mut events,hold);
    answer(&mut state,&mut events,"H-1","扩大路径",RememberAs::ModuleDecision{id:"D-1".into(),supersedes:vec![],review_on:None});
    assert!(state.decisions.is_empty());
    assert!(state.rules.is_empty());
    assert_eq!(state.proposals.len(),1);
    assert!(state.pending_messages("R-1").is_empty());
    assert!(matches!(state.wait_plan("H-1",1002,None).unwrap(),WaitPlan::Deferred{proposal:Some(_),..}));
}

#[test]
fn humans_only_and_rejected_answers_leave_no_partial_decisions_or_rules() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    open(&mut state,&mut events,question("H-1","R-1","A-a","格式？"));
    let forged=events.event("hold.answered","process:towermd-cli","H-1",QuestionEvent::Answered{
        answer:AnswerInput{hold:"H-1".into(),text:"JSON".into(),
            remember_as:RememberAs::ModuleDecision{id:"D-forged".into(),supersedes:vec![],review_on:None},targets:vec![],assumption:None},
    });
    assert!(state.apply(&forged).is_err());
    assert!(state.answers.is_empty());assert!(state.decisions.is_empty());
    answer(&mut state,&mut events,"H-1","JSON",RememberAs::Temporary);
    let before=state.clone();
    let replacement=events.event("hold.answered","human:local","H-1",QuestionEvent::Answered{
        answer:AnswerInput{hold:"H-1".into(),text:"other".into(),
            remember_as:RememberAs::ModuleDecision{id:"D-leaked".into(),supersedes:vec![],review_on:None},targets:vec![],assumption:None},
    });
    assert!(state.apply(&replacement).is_err());
    assert_eq!(state,before);
}

#[test]
fn scoped_prompts_have_per_run_delivery_and_reading_does_not_acknowledge() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    let active=vec![binding("R-1","A-a"),binding("R-2","A-a"),binding("R-3","A-b")];
    let prompt=prepare_prompt("P-1".into(),PromptScope::Scoped{scope:"A-a".into()},
        "请先报告".into(),&active,vec![]).unwrap();
    let queued=events.event("prompt.queued","human:local","P-1",QuestionEvent::PromptQueued{prompt});
    state.apply(&queued).unwrap();
    assert_eq!(state.pending_messages("R-1").len(),1);
    assert_eq!(state.pending_messages("R-1").len(),1);
    assert!(state.deliveries.is_empty());
    assert_eq!(state.pending_messages("R-2").len(),1);
    assert!(state.pending_messages("R-3").is_empty());
    let message=state.pending_messages("R-1")[0].message.clone();
    let delivered=events.event("prompt.delivered","process:towermd-cli","P-1",
        QuestionEvent::Delivered{delivery:Delivery{run:"R-1".into(),message:message.clone()}});
    state.apply(&delivered).unwrap();
    state.apply(&delivered).unwrap();
    assert!(state.pending_messages("R-1").is_empty());
    assert_eq!(state.pending_messages("R-2").len(),1);
    assert_eq!(state.deliveries.len(),1);
    assert_eq!(state.deliveries.values().next().unwrap().event,delivered.eid);
    let wrong=events.event("prompt.delivered","process:towermd-cli","P-1",
        QuestionEvent::Delivered{delivery:Delivery{run:"R-3".into(),message}});
    assert!(state.apply(&wrong).is_err());
}

#[test]
fn a_prompt_moves_queued_delivered_handled_per_recipient() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    let active=vec![binding("R-1","A-a"),binding("R-2","A-a")];
    let prompt=prepare_prompt("P-1".into(),PromptScope::Global,"先别改依赖".into(),&active,vec![]).unwrap();
    let queued=events.event("prompt.queued","human:local","P-1",QuestionEvent::PromptQueued{prompt});
    state.apply(&queued).unwrap();
    let delivery=|run:&str|Delivery{run:run.into(),message:MessageId::Prompt{id:"P-1".into()}};
    assert_eq!(state.prompt_state("P-1","R-1"),Some(PromptState::Queued));
    assert_eq!(state.prompt_state("P-1","R-9"),None);
    // Handling what never arrived is refused; the log stays unchanged.
    let early=events.event("prompt.handled","process:towermd-cli","P-1",QuestionEvent::Handled{delivery:delivery("R-1"),note:None});
    let before=state.clone();
    assert!(state.apply(&early).is_err());
    assert_eq!(state,before);
    let delivered=events.event("prompt.delivered","process:towermd-cli","P-1",QuestionEvent::Delivered{delivery:delivery("R-1")});
    state.apply(&delivered).unwrap();
    assert_eq!(state.prompt_state("P-1","R-1"),Some(PromptState::Delivered));
    let handled=events.event("prompt.handled","process:towermd-cli","P-1",
        QuestionEvent::Handled{delivery:delivery("R-1"),note:Some("已回退依赖改动".into())});
    state.apply(&handled).unwrap();
    state.apply(&handled).unwrap();
    assert_eq!(state.prompt_state("P-1","R-1"),Some(PromptState::Handled));
    assert_eq!(state.prompt_state("P-1","R-2"),Some(PromptState::Queued));
    assert_eq!(state.handled.values().next().unwrap().event,handled.eid);
    // A second, different receipt for the same delivery is a conflict.
    let again=events.event("prompt.handled","process:towermd-cli","P-1",QuestionEvent::Handled{delivery:delivery("R-1"),note:None});
    assert!(state.apply(&again).is_err());
    // Only the delegate's process signs it, and only prompts are handled.
    let human=events.event("prompt.handled","human:local","P-1",QuestionEvent::Handled{delivery:delivery("R-1"),note:None});
    assert!(state.clone().apply(&human).is_err());
    let answer=events.event("prompt.handled","process:towermd-cli","H-1",QuestionEvent::Handled{
        delivery:Delivery{run:"R-1".into(),message:MessageId::Answer{hold:"H-1".into(),event:"e".into()}},note:None});
    assert!(state.clone().apply(&answer).is_err());
    let long=events.event("prompt.handled","process:towermd-cli","P-1",QuestionEvent::Handled{delivery:delivery("R-2"),note:Some("x".repeat(HANDLED_NOTE_CHARS+1))});
    assert!(state.clone().apply(&long).is_err());
}

#[test]
fn the_handled_group_appears_only_after_the_first_handled_prompt() {
    use towermd_core::question_projection::{self,HANDLED};
    let mut state=serde_json::json!({});let mut events=Events::new();
    let prompt=prepare_prompt("P-1".into(),PromptScope::Global,"注意".into(),&[binding("R-1","A-a")],vec![]).unwrap();
    question_projection::apply(&mut state,&events.event("prompt.queued","human:local","P-1",QuestionEvent::PromptQueued{prompt})).unwrap();
    let delivery=Delivery{run:"R-1".into(),message:MessageId::Prompt{id:"P-1".into()}};
    question_projection::apply(&mut state,&events.event("prompt.delivered","process:towermd-cli","P-1",QuestionEvent::Delivered{delivery:delivery.clone()})).unwrap();
    assert!(state.get(HANDLED).is_none());
    question_projection::apply(&mut state,&events.event("prompt.handled","process:towermd-cli","P-1",QuestionEvent::Handled{delivery,note:None})).unwrap();
    assert_eq!(state[HANDLED].as_object().unwrap().len(),1);
    assert_eq!(question_projection::read(&state).unwrap().prompt_state("P-1","R-1"),Some(PromptState::Handled));
}

#[test]
fn answer_delivery_is_bound_to_original_answer_and_slow_prompts_are_not_delivered() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    open(&mut state,&mut events,question("H-1","R-1","A-a","格式？"));
    let answer=answer(&mut state,&mut events,"H-1","JSON",RememberAs::Temporary);
    let delivered=events.event("prompt.delivered","process:towermd-cli","H-1",QuestionEvent::Delivered{
        delivery:Delivery{run:"R-1".into(),message:MessageId::Answer{hold:"H-1".into(),event:answer.eid}},
    });
    state.apply(&delivered).unwrap();
    assert!(state.pending_messages("R-1").is_empty());
    let prompt=prepare_prompt("P-1".into(),PromptScope::Global,"改验收标准".into(),
        &[binding("R-1","A-a")],vec![ChangeTarget{id:"C-1".into(),layer:"L2".into()}]).unwrap();
    let queued=events.event("prompt.queued","human:local","P-1",QuestionEvent::PromptQueued{prompt});
    state.apply(&queued).unwrap();
    assert!(state.prompts.is_empty());assert_eq!(state.proposals.len(),1);
    assert!(state.pending_messages("R-1").is_empty());
}

#[test]
fn an_assumption_travels_with_the_answer_and_becomes_the_decision_rationale() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    open(&mut state,&mut events,question("H-1","R-1","A-a","格式？"));
    let assumed=|hold:&str,remember_as:RememberAs,assumption:&str|QuestionEvent::Answered{
        answer:AnswerInput{hold:hold.into(),text:"CSV".into(),remember_as,targets:vec![],assumption:Some(assumption.into())},
    };
    for bad in ["  ","两行\n假设",&"长".repeat(ASSUMPTION_CHARS+1)] {
        let event=events.event("hold.answered","human:local","H-1",assumed("H-1",RememberAs::Temporary,bad));
        assert!(state.apply(&event).is_err(),"{bad:?}");
    }
    let event=events.event("hold.answered","human:local","H-1",
        assumed("H-1",RememberAs::ModuleDecision{id:"D-1".into(),supersedes:vec![],review_on:None},"用户都用 Excel 打开"));
    state.apply(&event).unwrap();
    assert_eq!(state.decisions["D-1"].rationale.as_deref(),Some("用户都用 Excel 打开"));
    assert_eq!(state.pending_messages("R-1")[0].text,"CSV\n（假设：用户都用 Excel 打开）");
    let document=decision_document(&state.decisions["D-1"]).unwrap();
    assert_eq!(document.get_str("rationale"),Some("用户都用 Excel 打开"));
    let catalog=DecisionCatalog::from_projection(&state);
    let reuse=catalog.reuse_event(&question("H-2","R-2","A-a","格式？")).unwrap().unwrap();
    let receipt=events.event("hold.decision_reused","process:towermd-dedupe","H-2",reuse);
    state.apply(&receipt).unwrap();
    assert_eq!(state.pending_messages("R-2")[0].text,"CSV\n（假设：用户都用 Excel 打开）","a reused decision keeps its rationale");
}

fn git(root:&Path,args:&[&str])->String {
    let output=Command::new("git").current_dir(root)
        .env("GIT_CONFIG_GLOBAL",if cfg!(windows){"NUL"}else{"/dev/null"}).env("GIT_CONFIG_NOSYSTEM","1")
        .args(["-c","user.name=Questions Test","-c","user.email=qa@localhost","-c","commit.gpgsign=false"])
        .args(args).output().unwrap();
    assert!(output.status.success(),"{args:?}: {}",String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim_end().into()
}
fn decision(text:&str,status:&str)->String {
    format!("---\ntype: Decision\nid: D-main\nmodule: A-a\nlayer: L4\nclass: minor\nstatus: {status}\napproved_by: human:local\nquestion: 格式？\ndecision: {text}\n---\n")
}

#[test]
fn main_decision_reader_uses_fixed_authoritative_commit_not_dirty_or_feature_branch_content() {
    let temp=TempDir::new().unwrap();let root=temp.path();
    git(root,&["init","-qb","main"]);
    fs::create_dir_all(root.join("docs/tower/decisions")).unwrap();
    let path=root.join("docs/tower/decisions/D-main.md");
    fs::write(&path,decision("JSON","accepted")).unwrap();
    git(root,&["add","."]);git(root,&["commit","-qm","accepted main decision"]);
    let main=git(root,&["rev-parse","HEAD"]);
    git(root,&["checkout","-qb","t/experiment"]);
    fs::write(&path,decision("unapproved-feature","accepted")).unwrap();
    git(root,&["add","."]);git(root,&["commit","-qm","feature decision"]);
    fs::write(&path,decision("dirty-worktree","accepted")).unwrap();
    let mut catalog=DecisionCatalog::default();
    catalog.read_main(root,"docs/tower").unwrap();
    let DecisionMatch::Exact(reference)=catalog.lookup("A-a","格式?").unwrap()else{panic!("accepted main decision expected")};
    assert_eq!(reference.decision,"JSON");
    assert!(matches!(reference.source,DecisionSource::MainCommit{commit,..}if commit==main));
    let mut state=QuestionProjection::default();let mut events=Events::new();
    let question=question("H-1","R-1","A-a","格式?");
    let event=events.event("hold.decision_reused","process:towermd-dedupe","H-1",catalog.reuse_event(&question).unwrap().unwrap());
    state.apply(&event).unwrap();
    assert_eq!(state.answers["H-1"].input.text,"JSON");
}

#[test]
fn draft_main_decisions_and_malformed_question_metadata_do_not_auto_answer() {
    let temp=TempDir::new().unwrap();let root=temp.path();
    git(root,&["init","-qb","main"]);
    fs::create_dir_all(root.join("docs/tower/decisions")).unwrap();
    fs::write(root.join("docs/tower/decisions/D-main.md"),decision("JSON","draft")).unwrap();
    git(root,&["add","."]);git(root,&["commit","-qm","draft"]);
    let mut catalog=DecisionCatalog::default();catalog.read_main(root,"docs/tower").unwrap();
    assert_eq!(catalog.lookup("A-a","格式?").unwrap(),DecisionMatch::Missing);
    let mut state=QuestionProjection::default();let mut events=Events::new();
    let mut malformed=question("H-1","R-1","A-a","格式?");
    malformed.deadline+=1;
    let event=events.event("hold.opened","process:towermd-cli","H-1",QuestionEvent::Opened{question:malformed});
    assert!(state.apply(&event).is_err());assert!(state.holds.is_empty());
}

#[test]
fn a_question_is_signed_by_the_cli_or_by_the_delegate_itself() {
    for actor in ["process:towermd-cli","claude-code/2.1.3"] {
        let mut state=QuestionProjection::default();let mut events=Events::new();
        let opened=events.event("hold.opened",actor,"H-1",QuestionEvent::Opened{question:question("H-1","R-1","A-a","格式？")});
        state.apply(&opened).unwrap();
        assert!(state.holds.contains_key("H-1"),"{actor}");
    }
    for actor in ["human:local","process:towermd","process:towermd-dedupe"] {
        let mut state=QuestionProjection::default();let mut events=Events::new();
        let opened=events.event("hold.opened",actor,"H-1",QuestionEvent::Opened{question:question("H-1","R-1","A-a","格式？")});
        assert!(state.apply(&opened).is_err(),"{actor}");
    }
}

#[test]
fn relayed_answer_self_report_needs_a_named_source() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    let opened=events.event("hold.opened","process:towermd-cli","H-1",
        QuestionEvent::Opened{question:question("H-1","R-1","A-a","格式？")});
    state.apply(&opened).unwrap();
    let relayed=events.event("hold.answered","process:towermd-cli","H-1",QuestionEvent::AnswerRelayed{
        hold:"H-1".into(),text:"JSON".into(),relayed_from:"human".into(),
        self_reported:Some(SelfReported{name:"".into(),version:"1".into(),..Default::default()}),
    });
    let error=state.apply(&relayed).unwrap_err();
    assert!(error.contains("自述来源名称"),"{error}");
}

#[test]
fn scope_fields_keep_the_v0_1_1_wire_names() {
    let context=RunContext{run:"R-1".into(),task:"T-1".into(),scope:"A-a".into()};
    assert_eq!(serde_json::to_value(&context).unwrap(),serde_json::json!({"run":"R-1","task":"T-1","module":"A-a"}));
    let scope:PromptScope=serde_json::from_value(serde_json::json!({"kind":"module","module":"A-a"})).unwrap();
    assert_eq!(scope,PromptScope::Scoped{scope:"A-a".into()});
    assert_eq!(serde_json::to_value(&scope).unwrap(),serde_json::json!({"kind":"module","module":"A-a"}));
}

fn controller(hold:&str,text:&str,basis:&[&str])->QuestionEvent {
    QuestionEvent::ControllerAnswered{hold:hold.into(),text:text.into(),basis:basis.iter().map(|b|b.to_string()).collect(),notify:true}
}

/// FR-23/24/65: the controller answers a fast-layer question with its basis,
/// signed as itself; it cannot answer a slow-layer one or answer without basis.
#[test]
fn controller_answers_fast_questions_with_basis_and_never_slow_ones() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    open(&mut state,&mut events,question("H-1","R-1","A-a","格式？"));
    let unsigned=events.event("hold.answered","human:local","H-1",controller("H-1","用 JSON",&["验收标准 1"]));
    assert!(state.apply(&unsigned).is_err(),"a controller answer must be signed by the controller");
    let bare=events.event("hold.answered",CONTROLLER,"H-1",controller("H-1","用 JSON",&[]));
    assert!(state.apply(&bare).is_err(),"no basis, no answer");
    let accepted=events.event("hold.answered",CONTROLLER,"H-1",controller("H-1","用 JSON",&["验收标准：输出 JSON"]));
    state.apply(&accepted).unwrap();
    let answer=&state.answers["H-1"];
    assert_eq!((answer.actor.as_str(),answer.notify,answer.basis.len()),(CONTROLLER,true,1));
    assert!(matches!(state.wait_plan("H-1",1001,None).unwrap(),WaitPlan::Answered{..}));
    let mut slow=question("H-2","R-2","A-a","改接口？");
    slow.targets=vec![ChangeTarget{id:"A-a".into(),layer:"L2".into()}];
    open(&mut state,&mut events,slow);
    let refused=events.event("hold.answered",CONTROLLER,"H-2",controller("H-2","改",&["基线"]));
    assert!(state.apply(&refused).is_err(),"a slow-layer question is a person's");
    assert!(!state.answers.contains_key("H-2"));
}

/// FR-66: a person's answer replaces the controller's and records what it
/// replaced; the new answer is delivered as a new message. A relay cannot.
#[test]
fn a_person_overrides_a_controller_answer_and_the_new_answer_is_delivered() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    open(&mut state,&mut events,question("H-1","R-1","A-a","格式？"));
    let first=events.event("hold.answered",CONTROLLER,"H-1",controller("H-1","用 JSON",&["验收标准"]));
    state.apply(&first).unwrap();
    assert_eq!(state.pending_messages("R-1").len(),1);
    let relay=events.event("hold.answered","process:towermd-cli","H-1",
        QuestionEvent::AnswerRelayed{hold:"H-1".into(),text:"用 YAML".into(),relayed_from:"human".into(),self_reported:None});
    assert!(state.apply(&relay).is_err(),"only the App's person overrides");
    let human=answer(&mut state,&mut events,"H-1","用 YAML",RememberAs::Temporary);
    let replaced=&state.answers["H-1"];
    assert_eq!(replaced.actor,"human:local");
    assert_eq!(replaced.overrides.as_ref().map(|old|(old.event.as_str(),old.text.as_str())),Some((first.eid.as_str(),"用 JSON")));
    let pending=state.pending_messages("R-1");
    assert_eq!(pending.len(),1);
    assert_eq!(pending[0].message,MessageId::Answer{hold:"H-1".into(),event:human.eid.clone()});
    let again=events.event("hold.answered",CONTROLLER,"H-1",controller("H-1","还是 JSON",&["验收标准"]));
    assert!(state.apply(&again).is_err(),"the controller never overrides a person");
}
