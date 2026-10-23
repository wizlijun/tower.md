use super::*;
use std::{fs,process::Command};
use serde_json::json;
use crate::{dev_state, task_model::{Task,TaskDraft,TaskOrigin,TaskState}};
use towermd_core::{config::Config, hub::{Event,Fence}, main_writer::{self,WriteKind,WriteOutcome,WriteRequest}};
fn git(root:&Path,args:&[&str])->String{
    let output=Command::new("git").current_dir(root).env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1")
        .args(["-c","user.name=Proposal Test","-c","user.email=test@example.invalid","-c","commit.gpgsign=false","-c","core.hooksPath=/dev/null"])
        .args(args).output().unwrap();
    assert!(output.status.success(),"{args:?}: {}",String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn write(root:&Path,path:&str,text:&str){let path=root.join(path);fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,text).unwrap();}
fn commit(root:&Path)->String{git(root,&["add","."]);git(root,&["commit","-qm","test"]);git(root,&["rev-parse","HEAD"])}
fn fixture()->tempfile::TempDir{
    let temp=tempfile::tempdir().unwrap();let root=temp.path();git(root,&["init","-qb","main"]);
    write(root,".towermd/config.yaml",&Config::new("proposal-project","docs/tower").unwrap().to_yaml());
    for (id,path) in [("A","src/a/**"),("B","src/b/**"),("C","src/c/**")]{
        write(root,&format!("docs/tower/arch/{id}.md"),&format!("---\ntype: Module\nid: {id}\nlayer: L1\npaths: ['{path}']\n---\n{id}\n"));
        write(root,&path.replace("/**","/file.txt"),"original\n");
    }
    commit(root);temp
}
fn task(id:&str,module:&str,touches:&[&str])->Task{
    Task{input:TaskDraft{id:id.into(),title:id.into(),module:module.into(),touches:touches.iter().map(|s|s.to_string()).collect(),allowed_paths:vec![format!("src/{}/**",module.to_lowercase())],depends_on:vec![],acceptance:vec!["passes".into()],priority:0,decisions:vec![],..TaskDraft::default()},origin:TaskOrigin::Cli,baseline:"towermd/B0".into(),state:TaskState::Ready,needs_review:false,active_run:None}
}
fn projection()->TaskProjection{TaskProjection{tasks:BTreeMap::from([("T-a".into(),task("T-a","A",&[])),("T-b".into(),task("T-b","B",&[])),("T-c".into(),task("T-c","C",&["B"]))]),..TaskProjection::default()}}
fn event(eid:&str,kind:&str,actor:&str,layer:&str,obj:&str,data:serde_json::Value)->Event{
    serde_json::from_value(json!({"seq":1,"eid":eid,"host":"test","t":"2026-10-21T12:00:00Z","actor":actor,"type":kind,
        "layer":layer,"obj":obj,"data":data,"epoch":null,"stale":false,"cost_usd":null,"guards":[]})).unwrap()
}
fn submitted(catalog:&Catalog,impact:Impact)->Proposal{
    let mut proposal=Proposal{id:format!("P-{}",uuid::Uuid::now_v7()),project_id:catalog.config.project_id().into(),source_revision:catalog.revision.clone(),baseline_tag:"towermd/B0".into(),draft_id:Some(format!("d-{}","a".repeat(64))),source_proposal_id:None,source_commit:None,source_parent:None,kind:Kind::Interface,reason:"更新调用契约".into(),alternatives:vec!["保留原契约".into(),"采用新契约".into()],recommendation:"采用新契约".into(),impact,changes:vec![],sources:vec![],content_hash:String::new()};
    proposal.content_hash=proposal.hash().unwrap();proposal
}
#[test]
fn captures_real_complete_edge_without_changing_any_user_ref_or_index(){
    let temp=fixture();let root=temp.path();let catalog=Catalog::load(root).unwrap();
    git(root,&["checkout","-qb","t/T-a"]);
    write(root,"docs/tower/arch/A.md","---\ntype: Module\nid: A\nlayer: L1\npaths: ['src/a/**','src/new/**']\n---\nChanged\n");
    write(root,"outside.txt","unauthorized\n");let tip=commit(root);
    let guard=path_guard::check(root,&catalog.revision,&tip,"docs/tower",&["src/a/**".into()]).unwrap();assert!(!guard.passed);
    let index=fs::read(root.join(".git/index")).unwrap();let refs=git(root,&["show-ref"]);
    let proposal=prepare_from_changes(&catalog,&projection(),"towermd/B0",&tip,&catalog.revision,&guard.changes).unwrap();
    assert!(proposal.draft_id.is_none());assert_eq!(proposal.impact.modules,vec!["A"]);assert_eq!(proposal.impact.tasks,vec!["T-a"]);
    assert!(proposal.impact.paths.contains(&"src/new/**".into()));assert_eq!(proposal.changes.len(),2);
    assert!(proposal.changes.iter().any(|change|change.before.as_deref().is_some_and(|text|text.contains("src/a/**"))));
    assert_eq!(index,fs::read(root.join(".git/index")).unwrap());assert_eq!(refs,git(root,&["show-ref"]));
    let mut incomplete=guard.changes.clone();incomplete.pop();assert!(prepare_from_changes(&catalog,&projection(),"towermd/B0",&tip,&catalog.revision,&incomplete).is_err());
    let mut changed=proposal.clone();changed.reason.push('!');assert!(changed.validate().is_err());
    assert!(prepare_from_changes(&catalog,&projection(),"towermd/B0",&tip,&"f".repeat(40),&guard.changes).is_err());
}
#[test]
fn impact_covers_old_and_new_interface_endpoints_acceptance_and_task_touches(){
    let temp=fixture();let catalog=Catalog::load(temp.path()).unwrap();
    let changes=vec![Change{path:"docs/tower/arch/IF-call.md".into(),before_oid:None,after_oid:None,
        before:Some("---\ntype: Interface\nid: IF-call\nfrom: A\nto: B\n---\nold\n".into()),
        after:Some("---\ntype: Interface\nid: IF-call\nfrom: A\nto: C\n---\nnew\n".into())}];
    let impact=infer_impact(&catalog,None,&changes,&projection()).unwrap();
    assert_eq!(impact.modules,vec!["A","B","C"]);assert_eq!(impact.tasks,vec!["T-a","T-b","T-c"]);
    let changes=vec![Change{path:"docs/tower/specs/AC-b.md".into(),before_oid:None,after_oid:None,before:None,after:Some("---\ntype: Acceptance\nid: AC-b\nmodule: B\n---\nexpected\n".into())}];
    let impact=infer_impact(&catalog,None,&changes,&projection()).unwrap();assert_eq!(impact.modules,vec!["B"]);assert_eq!(impact.tasks,vec!["T-b","T-c"]);
    assert!(metadata_only("---\ntype: Module\nid: A\nbaseline: B0\nstatus: stable\n---\nbody\n","---\ntype: Module\nid: A\nbaseline: B1\nstatus: draft\n---\nbody\n"));
}
#[test]
fn proposal_approval_decision_and_tag_share_one_real_commit_and_only_affected_tasks_need_review(){
    let temp=fixture();let root=temp.path();let catalog=Catalog::load(root).unwrap();
    let proposal=submitted(&catalog,Impact{modules:vec!["B".into()],tasks:vec!["T-b".into(),"T-c".into()],..Impact::default()});
    let mut state=dev_state::initial();
    for (id,task) in projection().tasks{state["Task"][id]=serde_json::to_value(task).unwrap();}
    let submission=event(proposal.id.strip_prefix("P-").unwrap(),"proposal.submitted","process:towermd-cli","L4",&proposal.id,json!(proposal));
    dev_state::apply(&mut state,&submission).unwrap();
    let mut publication=Publication{request_id:uuid::Uuid::now_v7().to_string(),draft_id:proposal.draft_id.clone().unwrap(),project_id:proposal.project_id.clone(),source_revision:catalog.revision.clone(),target:"B1".into(),prepared_at:"2026-10-21T12:00:00Z".into(),content_hash:String::new(),removals:vec![],files:BTreeMap::from([("docs/tower/baselines/B1.md".into(),b"---\ntype: Baseline\nid: B1\nobjects: {}\n---\n".to_vec())])};
    publication.content_hash=publication.hash().unwrap();let old_hash=publication.content_hash.clone();assert!(publication_link(&publication).unwrap().is_none());
    attach_to_publication(&mut publication,&proposal,"docs/tower").unwrap();assert_ne!(publication.content_hash,old_hash);
    assert_eq!(publication_link(&publication).unwrap().unwrap().affected_modules,vec!["B"]);
    let approval=event(&publication.request_id,"baseline.approved","human:local","L1","B1",json!(publication));
    let mut forged=approval.clone();forged.actor="process:towermd-cli".into();assert!(dev_state::apply(&mut state.clone(),&forged).is_err());
    dev_state::apply(&mut state,&approval).unwrap();
    git(root,&["checkout","-qb","working"]);let index=fs::read(root.join(".git/index")).unwrap();
    let outcome=main_writer::write(root,&WriteRequest{docs_dir:"docs/tower".into(),epoch:4,kind:WriteKind::Slow{expected_base:catalog.revision.clone(),tag:Some("refs/tags/towermd/B1".into())},changes:publication.files.iter().map(|(p,b)|(p.clone(),Some(b.clone()))).collect(),message:format!("approve B1\n\nTowermd-Baseline: B1\nTowermd-Approval: {}\nTowermd-Content: {}",approval.eid,publication.content_hash)},||Ok(())).unwrap();
    let WriteOutcome::Written{commit,..}=outcome else{panic!("not written")};
    publication.prove_commit(root,&commit,&approval).unwrap();
    assert_eq!(git(root,&["rev-parse","towermd/B1"]),commit);assert_eq!(git(root,&["rev-parse","HEAD"]),catalog.revision);assert_eq!(index,fs::read(root.join(".git/index")).unwrap());
    let mut created=event("created","baseline.created","process:towermd","L1","B1",json!({"commit":commit,"approval_eid":approval.eid,"content_hash":publication.content_hash}));
    created.guards=vec![Fence{name:"main".into(),holder:"daemon".into(),epoch:4}];created.epoch=Some(4);
    dev_state::apply(&mut state,&created).unwrap();
    assert_eq!(state["Proposal"][&proposal.id]["status"],"approved");
    let tasks=dev_state::tasks(&state).unwrap();assert!(!tasks.tasks["T-a"].needs_review);assert_eq!(tasks.tasks["T-a"].baseline,"towermd/B1");
    assert!(tasks.tasks["T-b"].needs_review);assert!(tasks.tasks["T-c"].needs_review);
    let reject=event("rejection","proposal.rejected","human:local","L4",&proposal.id,json!({"proposal_hash":proposal.content_hash,"reason":"changed mind"}));
    assert!(dev_state::apply(&mut state,&reject).is_err());
}
#[test]
fn self_reported_approval_or_incomplete_capture_cannot_publish(){
    let temp=fixture();let catalog=Catalog::load(temp.path()).unwrap();let proposal=submitted(&catalog,Impact::default());
    let mut state=dev_state::initial();let mut event=event(proposal.id.strip_prefix("P-").unwrap(),"proposal.submitted","process:towermd-cli","L4",&proposal.id,json!(proposal));
    event.actor="human:local".into();assert!(apply(&mut state,&event).is_err());event.actor="process:towermd-cli".into();apply(&mut state,&event).unwrap();
    let rejection=event_reject(&proposal);let mut forged=rejection.clone();forged.actor="process:towermd-cli".into();assert!(apply(&mut state,&forged).is_err());
    apply(&mut state,&rejection).unwrap();assert_eq!(state["Proposal"][&proposal.id]["status"],"rejected");
    let mut capture=proposal.clone();capture.draft_id=None;capture.content_hash=capture.hash().unwrap();
    let mut publication=Publication{request_id:uuid::Uuid::now_v7().to_string(),draft_id:proposal.draft_id.unwrap(),project_id:proposal.project_id,source_revision:proposal.source_revision,target:"B1".into(),prepared_at:"2026-10-21T12:00:00Z".into(),content_hash:String::new(),removals:vec![],files:BTreeMap::new()};
    assert!(attach_to_publication(&mut publication,&capture,"docs/tower").is_err());
}
fn event_reject(proposal:&Proposal)->Event{event("reject","proposal.rejected","human:local","L4",&proposal.id,json!({"proposal_hash":proposal.content_hash,"reason":"保留原方案"}))}

#[test]
fn approved_a_b_baseline_merge_removes_b_from_git_and_marks_its_task_for_review(){
    let temp=fixture();let root=temp.path();let original=Catalog::load(root).unwrap();git(root,&["checkout","-qb","working"]);
    let mut files=BTreeMap::new();let mut objects=serde_json::Map::new();
    for id in ["A","B","C"]{
        let path=format!("docs/tower/arch/{id}.md");let bytes=fs::read(root.join(&path)).unwrap();
        objects.insert(id.into(),json!({"path":path,"sha256":format!("{:x}",Sha256::digest(&bytes))}));files.insert(path,bytes);
    }
    let mut manifest=Document::new("Baseline","approved B0\n").unwrap();manifest.set("objects",serde_yaml::to_value(objects).unwrap()).unwrap();
    files.insert("docs/tower/baselines/B0.md".into(),manifest.to_text().into_bytes());
    let mut initial=Publication{request_id:uuid::Uuid::now_v7().to_string(),draft_id:format!("d-{}","b".repeat(64)),project_id:original.config.project_id().into(),source_revision:original.revision.clone(),target:"B0".into(),prepared_at:"2026-10-21T12:00:00Z".into(),content_hash:String::new(),files,removals:vec![]};
    initial.content_hash=initial.hash().unwrap();let approve0=event(&initial.request_id,"baseline.approved","human:local","L1","B0",json!(initial));
    let write_publication=|publication:&Publication|{
        let outcome=main_writer::write(root,&WriteRequest{docs_dir:"docs/tower".into(),epoch:4,kind:WriteKind::Slow{expected_base:publication.source_revision.clone(),tag:Some(format!("refs/tags/towermd/{}",publication.target))},changes:publication.files.iter().map(|(p,b)|(p.clone(),Some(b.clone()))).chain(publication.removals.iter().map(|p|(p.clone(),None))).collect(),message:format!("Approve {}",publication.target)},||Ok(())).unwrap();
        let WriteOutcome::Written{commit,..}=outcome else{panic!("not written")};commit
    };
    let base=write_publication(&initial);
    let verified=crate::baseline_publication::verified_baseline(root,&base,&approve0).unwrap();
    let catalog=Catalog::load(root).unwrap();
    let mut next=BTreeMap::from([("docs/tower/arch/A.md".into(),b"---\ntype: Module\nid: A\npaths: ['src/a/**','src/b/**']\n---\nMerged\n".to_vec()),("docs/tower/arch/C.md".into(),initial.files["docs/tower/arch/C.md"].clone())]);
    let removed=retired_objects(&catalog,&verified,&next).unwrap();assert_eq!(removed.len(),1);assert_eq!(removed[0].0,"docs/tower/arch/B.md");
    let changes=removed.into_iter().map(|(path,id,text)|Change{path,before_oid:Some(id),after_oid:None,before:Some(text),after:None}).collect::<Vec<_>>();
    let mut proposal=submitted(&catalog,infer_impact(&catalog,None,&changes,&projection()).unwrap());proposal.changes=changes;proposal.content_hash=proposal.hash().unwrap();
    assert!(proposal.impact.modules.contains(&"B".into()));
    next.insert("docs/tower/baselines/B1.md".into(),b"---\ntype: Baseline\nid: B1\nobjects: {}\n---\n".to_vec());
    let mut publication=Publication{request_id:uuid::Uuid::now_v7().to_string(),draft_id:proposal.draft_id.clone().unwrap(),project_id:proposal.project_id.clone(),source_revision:base,target:"B1".into(),prepared_at:initial.prepared_at.clone(),content_hash:String::new(),files:next,removals:vec![]};
    attach_to_publication(&mut publication,&proposal,"docs/tower").unwrap();assert_eq!(publication.removals,vec!["docs/tower/arch/B.md"]);
    let approve1=event(&publication.request_id,"baseline.approved","human:local","L1","B1",json!(publication));
    let published=write_publication(&publication);publication.prove_commit(root,&published,&approve1).unwrap();
    assert!(git(root,&["ls-tree",&published,"--","docs/tower/arch/B.md"]).is_empty());
    let mut state=dev_state::initial();for(id,task)in projection().tasks{state["Task"][id]=json!(task);}
    let submission_event=event(proposal.id.strip_prefix("P-").unwrap(),"proposal.submitted","process:towermd-cli","L4",&proposal.id,json!(proposal));dev_state::apply(&mut state,&submission_event).unwrap();dev_state::apply(&mut state,&approve1).unwrap();
    let mut created=event("created-merged","baseline.created","process:towermd","L1","B1",json!({"commit":published,"approval_eid":approve1.eid,"content_hash":publication.content_hash}));created.guards=vec![Fence{name:"main".into(),holder:"daemon".into(),epoch:4}];created.epoch=Some(4);dev_state::apply(&mut state,&created).unwrap();
    assert!(dev_state::tasks(&state).unwrap().tasks["T-b"].needs_review);
    let mut forged=publication.clone();forged.removals.clear();forged.content_hash=forged.hash().unwrap();let forged_approval=event(&forged.request_id,"baseline.approved","human:local","L1","B1",json!(forged));assert!(forged.prove_commit(root,&published,&forged_approval).is_err());
    // The B1 proposal ADR remains authoritative in B2, with original bytes
    // and baseline signature, while the new ADR identifies only B2's proposal.
    let verified1=crate::baseline_publication::verified_baseline(root,&published,&approve1).unwrap();
    let catalog1=Catalog::load(root).unwrap();let mut proposal2=submitted(&catalog1,Impact::default());
    proposal2.baseline_tag="towermd/B1".into();proposal2.content_hash=proposal2.hash().unwrap();
    let mut files2=publication.files.clone();files2.retain(|path,_|!path.contains("/baselines/")&&!path.contains("/decisions/"));
    files2.insert("docs/tower/baselines/B2.md".into(),b"---\ntype: Baseline\nid: B2\nobjects: {}\n---\n".to_vec());
    let mut publication2=Publication{request_id:uuid::Uuid::now_v7().to_string(),draft_id:proposal2.draft_id.clone().unwrap(),project_id:proposal2.project_id.clone(),source_revision:published.clone(),target:"B2".into(),prepared_at:initial.prepared_at.clone(),content_hash:String::new(),files:files2,removals:vec![]};
    inherit_decisions(&mut publication2,&catalog1,&verified1).unwrap();attach_to_publication(&mut publication2,&proposal2,"docs/tower").unwrap();
    let inherited_path=format!("docs/tower/decisions/D-{}.md",proposal.id);
    assert_eq!(publication2.files[&inherited_path],publication.files[&inherited_path]);
    assert_eq!(publication_link(&publication2).unwrap().unwrap().id,proposal2.id);
    let approval2=event(&publication2.request_id,"baseline.approved","human:local","L1","B2",json!(publication2));
    let published2=write_publication(&publication2);let verified2=crate::baseline_publication::verified_baseline(root,&published2,&approval2).unwrap();
    let mut dependent=task("T-decision","A",&[]).input;dependent.decisions=vec![format!("D-{}",proposal.id)];
    let mut tasks2=TaskProjection::default();let created=crate::task_model::prepare_add(&verified2,&tasks2,dependent,TaskOrigin::Cli).unwrap();tasks2.apply(&created).unwrap();
    crate::task_model::check_g0(&verified2,&tasks2,"T-decision").unwrap();
    git(root,&["checkout","--detach","-q",&published2]);
    write(root,"docs/tower/decisions/D-rogue.md","---\ntype: Decision\nid: D-rogue\nlayer: L1\n---\nNot approved\n");let rogue=commit(root);git(root,&["update-ref","refs/heads/main",&rogue]);
    let catalog2=Catalog::load(root).unwrap();let mut probe=publication2.clone();probe.source_revision=rogue;probe.target="B3".into();probe.files.retain(|path,_|!path.contains("/baselines/")&&!path.contains("/decisions/"));
    probe.files.insert("docs/tower/baselines/B3.md".into(),b"---\ntype: Baseline\nid: B3\nobjects: {}\n---\n".to_vec());
    inherit_decisions(&mut probe,&catalog2,&verified2).unwrap();assert!(!probe.files.contains_key("docs/tower/decisions/D-rogue.md"));
    assert!(serde_json::to_value(&initial).unwrap().get("removals").is_none());
    assert_eq!(initial.hash().unwrap(),digest(&("baseline-publication-v1",&initial.request_id,&initial.draft_id,&initial.project_id,&initial.source_revision,&initial.target,&initial.prepared_at,&initial.files)).unwrap());
}

#[test]
fn submitted_diff_comes_from_real_scan_and_rejects_stale_source_or_forged_impact(){
    use crate::{baseline, cold_start, task_model::{PublishedBaseline,ModuleScope}, verifier::{self,Limits,VerificationPlan}};
    let temp=fixture();let root=temp.path();let mut manifest=Document::new("Baseline","test approved manifest\n").unwrap();
    for name in ["a","b","c"]{write(root,&format!("src/{name}/code.rs"),"fn example() {}\n");}
    let objects=["A","B","C"].into_iter().map(|id|{let path=format!("docs/tower/arch/{id}.md");let data=fs::read(root.join(&path)).unwrap();(id.to_string(),json!({"path":path,"sha256":format!("{:x}",Sha256::digest(data))}))}).collect::<BTreeMap<_,_>>();
    manifest.set("objects",serde_yaml::to_value(objects).unwrap()).unwrap();write(root,"docs/tower/baselines/B0.md",&manifest.to_text());let base=commit(root);
    let catalog=Catalog::load(root).unwrap();
    // This crate-private DTO is only used to isolate proposal input checks; the
    // previous test exercises the production approval/Git proof constructor.
    let baseline=VerifiedBaseline::from_published(PublishedBaseline{tag:"towermd/B0".into(),commit:base,approval_eid:"approved".into(),content_hash:"a".repeat(64),modules:["A","B","C"].into_iter().map(|id|(id.into(),ModuleScope{paths:vec![format!("src/{}/**",id.to_lowercase())]})).collect(),decisions:BTreeSet::new()}).unwrap();
    let mut scan=cold_start::scan(&catalog,&uuid::Uuid::now_v7().to_string()).unwrap();
    let report=verifier::run(&VerificationPlan{root:root.into(),id:scan.id.clone(),revision:scan.revision.clone(),tree:scan.tree.clone(),commands:scan.commands.clone(),modules:scan.modules.clone(),limits:Limits::default()},&std::sync::atomic::AtomicBool::new(false)).unwrap();cold_start::apply_verification(&mut scan,report).unwrap();
    let submission=baseline::template(&scan).unwrap();
    let draft=baseline::synthesize(&scan,submission,"test/1","2026-10-21T12:00:00Z",&format!("d-{}","c".repeat(64))).unwrap();
    let input=Input{draft_id:draft.id.clone(),source_proposal_id:None,kind:Kind::Module,reason:"根据当前代码调整模块".into(),alternatives:vec!["保持现状".into()],recommendation:"采用草稿".into(),sources:vec![]};
    let id=format!("P-{}",uuid::Uuid::now_v7());let proposal=prepare(&catalog,&draft,&projection(),&baseline,&id,input.clone()).unwrap();
    assert!(!proposal.changes.is_empty());assert!(proposal.changes.iter().all(|change|change.before_oid.is_some()||change.after_oid.is_some()));
    let mut forged=serde_json::to_value(input.clone()).unwrap();forged["impact"]=json!({"modules":[]});assert!(serde_json::from_value::<Input>(forged).is_err());
    let mut mismatched=draft.clone();mismatched.scan.revision="f".repeat(40);assert!(prepare(&catalog,&mismatched,&projection(),&baseline,&id,input.clone()).is_err());
    write(root,"new.txt","main advances\n");commit(root);assert!(prepare(&catalog,&draft,&projection(),&baseline,&id,input).is_err());
}
#[test]
fn note_md_sources_reach_the_adr_and_leave_old_hashes_alone(){
    let temp=fixture();let catalog=Catalog::load(temp.path()).unwrap();
    let plain=submitted(&catalog,Impact{modules:vec!["B".into()],..Impact::default()});
    // Without sources the hash keeps its exact v1 tuple, so recorded proposals replay unchanged.
    assert_eq!(plain.content_hash,digest(&("proposal-v1",&plain.id,&plain.project_id,&plain.source_revision,&plain.baseline_tag,
        &plain.draft_id,&plain.source_proposal_id,&plain.source_commit,&plain.source_parent,
        plain.kind,&plain.reason,&plain.alternatives,&plain.recommendation,&plain.impact,&plain.changes)).unwrap());
    assert!(!serde_json::to_string(&plain).unwrap().contains("sources"));
    let mut cited=plain.clone();
    cited.sources=vec![NoteRef::from_citation("((kb/raft.md#b-00ff00))").unwrap()];
    assert!(cited.validate().is_err(),"sources are part of the content hash");
    cited.content_hash=cited.hash().unwrap();cited.validate().unwrap();
    assert_ne!(cited.content_hash,plain.content_hash);
    let mut bad=cited.clone();bad.sources[0].block=Some("b-XYZ".into());bad.content_hash=bad.hash().unwrap();
    assert!(bad.validate().is_err());
    let input:Input=serde_json::from_value(json!({"draft_id":"d-x","kind":"other","reason":"r","alternatives":["a"],"recommendation":"r",
        "sources":[{"okf_id":"kb/raft","block":"b-00ff00"}]})).unwrap();
    assert_eq!(input.sources[0].okf_id,"kb/raft");
    let mut publication=Publication{request_id:uuid::Uuid::now_v7().to_string(),draft_id:cited.draft_id.clone().unwrap(),project_id:cited.project_id.clone(),source_revision:catalog.revision.clone(),target:"B1".into(),prepared_at:"2026-10-21T12:00:00Z".into(),content_hash:String::new(),removals:vec![],files:BTreeMap::from([("docs/tower/baselines/B1.md".into(),b"---\ntype: Baseline\nid: B1\nobjects: {}\n---\n".to_vec())])};
    publication.content_hash=publication.hash().unwrap();
    attach_to_publication(&mut publication,&cited,"docs/tower").unwrap();
    let adr=Document::parse(std::str::from_utf8(&publication.files[&format!("docs/tower/decisions/D-{}.md",cited.id)]).unwrap()).unwrap();
    assert_eq!(notemd::note_refs(&adr),cited.sources);
    let entry=&adr.get("sources").unwrap().as_sequence().unwrap()[0];
    assert_eq!(entry.get("resource").unwrap(),"kb/raft.md");
}
