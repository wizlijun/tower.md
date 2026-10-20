//! Questions, remembered answers and per-run delivery. This model never renews
//! a lease, changes a slow object, or treats a draft proposal as authorization.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::{hub::Event, okf::Document, repo};

pub const ASK_WAIT_MS: u64 = 90_000;
pub const LIVE_WAIT_MAX_MS: u64 = 600_000;

pub fn normalize_question(text: &str) -> String {
    let normalized: String = text.nfkc().collect::<String>().split_whitespace()
        .collect::<Vec<_>>().join(" ").chars().map(|c| c.to_ascii_lowercase()).collect();
    normalized.trim_end_matches(|c: char| c.is_whitespace() || punctuation(c)).to_owned()
}

pub fn fingerprint(module: &str, text: &str) -> Result<String, String> {
    valid_id(module)?;
    let normalized = normalize_question(text);
    nonempty(&normalized, "问题")?;
    let bytes = serde_json::to_vec(&(module, normalized)).map_err(|e| e.to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunContext {
    pub run: String,
    pub task: String,
    pub module: String,
}

/// Resolved by the daemon against the object catalog, never inferred from an
/// untrusted client-supplied layer string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeTarget {
    pub id: String,
    pub layer: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitMode { #[default] Live, Parked }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionDraft {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub recommended: Option<usize>,
    #[serde(default)]
    pub context: Vec<String>,
    #[serde(default)]
    pub mode: WaitMode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub draft: QuestionDraft,
    pub binding: RunContext,
    pub fingerprint: String,
    pub opened_at: u64,
    pub deadline: u64,
    pub targets: Vec<ChangeTarget>,
}

pub fn prepare_question(
    draft: QuestionDraft, binding: RunContext, targets: Vec<ChangeTarget>, now: u64,
) -> Result<Question, String> {
    valid_id(&draft.id)?;
    check_binding(&binding)?;
    nonempty(&draft.text, "问题")?;
    if draft.text.len() > 32 * 1024 || draft.options.len() > 16
        || draft.options.iter().any(|option| option.trim().is_empty())
        || draft.recommended.is_some_and(|index| index >= draft.options.len())
    { return Err("问题选项、推荐项或长度无效".into()); }
    check_targets(&targets)?;
    for path in &draft.context { safe_path(path)?; }
    Ok(Question {
        fingerprint: fingerprint(&binding.module, &draft.text)?, draft, binding,
        opened_at: now, deadline: now.checked_add(LIVE_WAIT_MAX_MS).ok_or("问题等待期限溢出")?,
        targets,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RememberAs {
    Temporary,
    ModuleDecision { id: String, #[serde(default)] supersedes: Vec<String> },
    ProcessRule { id: String },
    SlowProposal { id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerInput {
    pub hold: String,
    pub text: String,
    pub remember_as: RememberAs,
    #[serde(default)]
    pub targets: Vec<ChangeTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub input: AnswerInput,
    pub event: String,
    pub actor: String,
    pub run: String,
    pub answered_at: String,
    pub proposal: Option<String>,
    pub decision: Option<DecisionReference>,
}
impl Answer { pub fn effective(&self) -> bool { self.proposal.is_none() } }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionSource {
    AcceptedEvent { eid: String },
    MainCommit { commit: String, path: String, sha256: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionReference {
    pub id: String,
    pub module: String,
    pub question: String,
    pub fingerprint: String,
    pub decision: String,
    pub accepted_by: String,
    pub source: DecisionSource,
    #[serde(default)]
    pub supersedes: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct DecisionCatalog {
    decisions: BTreeMap<String, DecisionReference>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecisionMatch {
    Missing,
    Exact(DecisionReference),
    Conflict(Vec<String>),
}

pub fn decision_document(decision:&DecisionReference)->Result<Document,String> {
    valid_id(&decision.id)?;
    if !human_actor(&decision.accepted_by)
        || fingerprint(&decision.module,&decision.question)?!=decision.fingerprint
    {return Err("决策来源或问题指纹无效".into());}
    nonempty(&decision.decision,"决策")?;
    let mut document=Document::new("Decision",&format!("{}\n",decision.decision))?;
    for (key,value) in [
        ("id",decision.id.as_str()),("module",decision.module.as_str()),
        ("layer","L4"),("class","minor"),("status","accepted"),
        ("question",decision.question.as_str()),("decision",decision.decision.as_str()),
        ("approved_by",decision.accepted_by.as_str()),("question_fingerprint",decision.fingerprint.as_str()),
    ] {document.set(key,serde_yaml::Value::from(value))?;}
    document.set("supersedes",serde_yaml::to_value(&decision.supersedes).map_err(|e|e.to_string())?)?;
    document.set("answer_source",serde_yaml::to_value(&decision.source).map_err(|e|e.to_string())?)?;
    Ok(document)
}

impl DecisionCatalog {
    /// The projection must come from canonical accepted events, never a client.
    pub fn from_projection(projection: &QuestionProjection) -> Self {
        Self { decisions: projection.decisions.clone() }
    }

    pub fn lookup(&self, module: &str, question: &str) -> Result<DecisionMatch, String> {
        let fp = fingerprint(module, question)?;
        let superseded: BTreeSet<_> = self.decisions.values().filter(|d|d.module==module)
            .flat_map(|d| d.supersedes.iter()).collect();
        let matches: Vec<_> = self.decisions.values().filter(|decision|
            decision.module == module && decision.fingerprint == fp && !superseded.contains(&decision.id)).collect();
        let Some(first) = matches.first() else { return Ok(DecisionMatch::Missing) };
        if matches.iter().any(|decision| decision.decision != first.decision) {
            return Ok(DecisionMatch::Conflict(matches.iter().map(|decision| decision.id.clone()).collect()));
        }
        Ok(DecisionMatch::Exact((*first).clone()))
    }

    /// A conflict or a slow-layer question goes to the person, never an
    /// arbitrary winner. Call this before producing hold.opened.
    pub fn reuse_event(&self,question:&Question)->Result<Option<QuestionEvent>,String> {
        check_question(question)?;
        if slow(&question.targets) {return Ok(None);}
        Ok(match self.lookup(&question.binding.module,&question.draft.text)? {
            DecisionMatch::Exact(decision)=>Some(QuestionEvent::DecisionReused{question:question.clone(),decision}),
            DecisionMatch::Missing|DecisionMatch::Conflict(_)=>None,
        })
    }

    /// Reads only accepted L4 decisions from the fetched authoritative main,
    /// never dirty working-tree answers or an arbitrary user-supplied revision.
    pub fn read_main(&mut self, root: &Path, docs_dir: &str) -> Result<(), String> {
        safe_path(docs_dir)?;
        let remotes = repo::git_timeout(root, &["remote"], Duration::from_secs(30))?;
        let reference = if remotes.lines().any(|name| name == "origin") {
            "refs/remotes/origin/main"
        } else { "refs/heads/main" };
        let commit = repo::git_timeout(root, &["rev-parse", "--verify", reference], Duration::from_secs(30))?;
        let prefix = format!("{docs_dir}/decisions/");
        let listing = repo::git_bytes_timeout(root, &["ls-tree", "-r", "-z", &commit, "--", &prefix], Duration::from_secs(30))?;
        for row in listing.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
            let row = std::str::from_utf8(row).map_err(|_| "决策路径不是 UTF-8")?;
            let (metadata, path) = row.split_once('\t').ok_or("决策 tree 条目无效")?;
            if !path.ends_with(".md") { continue; }
            if !metadata.starts_with("100644 blob ") { return Err("main 决策必须是普通 Markdown 文件".into()); }
            safe_path(path)?;
            let object = format!("{commit}:{path}");
            let size: usize = repo::git_timeout(root, &["cat-file","-s",&object],Duration::from_secs(30))?
                .parse().map_err(|_| "决策原件大小无效")?;
            if size > 1024 * 1024 { return Err("决策原件超过 1 MiB".into()); }
            let bytes = repo::git_bytes_timeout(root, &["cat-file","blob",&object],Duration::from_secs(30))?;
            let document = Document::parse(std::str::from_utf8(&bytes).map_err(|_| "决策不是 UTF-8")?)?;
            if document.doc_type() != Some("Decision") || document.get_str("status") != Some("accepted")
                || document.get_str("layer") != Some("L4") || document.get_str("class") != Some("minor")
            { continue; }
            let id = document.get_str("id").ok_or("已接受决策缺少 id")?.to_owned();
            let module = document.get_str("module").ok_or("已接受决策缺少 module")?.to_owned();
            let question = document.get_str("question").ok_or("已接受决策缺少精确问题文本")?.to_owned();
            let decision = document.get_str("decision").ok_or("已接受决策缺少 decision")?.to_owned();
            let accepted_by = document.get_str("approved_by").ok_or("已接受决策缺少人工批准来源")?.to_owned();
            if !human_actor(&accepted_by) {
                return Err("main 决策缺少人工批准来源".into());
            }
            valid_id(&id)?;
            nonempty(&decision, "决策")?;
            let supersedes = match document.get("supersedes") {
                None => vec![],
                Some(value) => serde_yaml::from_value::<Vec<String>>(value.clone()).map_err(|e|e.to_string())?,
            };
            for replaced in &supersedes { valid_id(replaced)?; }
            let accepted = DecisionReference {
                id: id.clone(), fingerprint: fingerprint(&module,&question)?, module, question, decision, accepted_by,
                source: DecisionSource::MainCommit {
                    commit:commit.clone(),path:path.into(),sha256:hex::encode(Sha256::digest(&bytes)),
                }, supersedes,
            };
            if let Some(old) = self.decisions.get(&id) {
                if old.module != accepted.module || old.question != accepted.question || old.decision != accepted.decision {
                    return Err("main 与本地已接受决策同 ID 内容冲突".into());
                }
            } else { self.decisions.insert(id, accepted); }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PromptScope { Global, Module { module: String }, Task { task: String } }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    pub id: String,
    pub scope: PromptScope,
    pub text: String,
    /// Frozen by the daemon from actual active runs when the person sends it.
    pub recipients: Vec<RunContext>,
    pub targets: Vec<ChangeTarget>,
}

pub fn prepare_prompt(
    id: String, scope: PromptScope, text: String, active: &[RunContext], targets: Vec<ChangeTarget>,
) -> Result<Prompt,String> {
    valid_id(&id)?;
    nonempty(&text,"插话")?;
    check_targets(&targets)?;
    match &scope { PromptScope::Module{module}=>valid_id(module)?, PromptScope::Task{task}=>valid_id(task)?, _=>{} }
    let mut recipients = Vec::new();
    let mut runs = BTreeSet::new();
    for binding in active {
        check_binding(binding)?;
        let matches = match &scope {
            PromptScope::Global => true,
            PromptScope::Module{module} => module==&binding.module,
            PromptScope::Task{task} => task==&binding.task,
        };
        if matches && runs.insert(binding.run.clone()) { recipients.push(binding.clone()); }
    }
    recipients.sort_by(|a,b|a.run.cmp(&b.run));
    Ok(Prompt{id,scope,text,recipients,targets})
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MessageId { Answer { hold: String, event: String }, Prompt { id: String } }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub run: String,
    pub message: MessageId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryReceipt {
    pub delivery: Delivery,
    pub event: String,
    pub delivered_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedMessage {
    pub message: MessageId,
    pub run: String,
    pub text: String,
    pub actor: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftProposal {
    pub id: String,
    pub source_event: String,
    pub text: String,
    pub targets: Vec<ChangeTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredRule {
    pub id: String,
    pub module: String,
    pub text: String,
    pub registered_by: String,
    pub event: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuestionEvent {
    Opened { question: Question },
    Answered { answer: AnswerInput },
    DecisionReused { question: Question, decision: DecisionReference },
    PromptQueued { prompt: Prompt },
    Delivered { delivery: Delivery },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionProjection {
    pub holds: BTreeMap<String, Question>,
    pub answers: BTreeMap<String, Answer>,
    pub decisions: BTreeMap<String, DecisionReference>,
    pub prompts: BTreeMap<String, Prompt>,
    pub prompt_actors: BTreeMap<String, String>,
    pub deliveries: BTreeMap<String, DeliveryReceipt>,
    pub proposals: BTreeMap<String, DraftProposal>,
    pub rules: BTreeMap<String, RegisteredRule>,
}

impl QuestionProjection {
    pub fn apply(&mut self, event: &Event) -> Result<(),String> {
        if event.stale { return Ok(()); }
        let input:QuestionEvent=serde_json::from_value(event.data.clone()).map_err(|e|e.to_string())?;
        let mut next=self.clone();
        next.apply_inner(event,input)?;
        *self=next;
        Ok(())
    }

    fn apply_inner(&mut self,event:&Event,input:QuestionEvent)->Result<(),String> {
        match input {
            QuestionEvent::Opened{question} => {
                envelope(event,"hold.opened","process:towermd-cli",&question.draft.id)?;
                check_question(&question)?;
                insert(&mut self.holds,&question.draft.id,question.clone())?;
            }
            QuestionEvent::Answered{mut answer} => {
                envelope(event,"hold.answered","human:local",&answer.hold)?;
                let question=self.holds.get(&answer.hold).ok_or("回答缺少已接受问题")?.clone();
                nonempty(&answer.text,"回答")?;
                check_targets(&answer.targets)?;
                let mut targets=question.targets.clone();
                for target in &answer.targets {
                    if let Some(old)=targets.iter().find(|old|old.id==target.id) {
                        if old.layer!=target.layer {return Err("影响对象层级与原问题不一致".into());}
                    } else {targets.push(target.clone());}
                }
                if slow(&targets) && !matches!(answer.remember_as,RememberAs::SlowProposal{..}) {
                    answer.remember_as=RememberAs::SlowProposal{id:format!("P-{}",event.eid)};
                }
                let mut accepted=Answer{input:answer.clone(),event:event.eid.clone(),actor:event.actor.clone(),
                    run:question.binding.run.clone(),answered_at:event.t.clone(),proposal:None,decision:None};
                match &answer.remember_as {
                    RememberAs::Temporary=>{}
                    RememberAs::ModuleDecision{id,supersedes}=>{
                        valid_id(id)?;
                        for replaced in supersedes {
                            valid_id(replaced)?;
                            if replaced==id || self.decisions.get(replaced).is_some_and(|old|old.module!=question.binding.module) {
                                return Err("模块决策不能替代自身或其他模块决策".into());
                            }
                        }
                        let decision=DecisionReference{id:id.clone(),module:question.binding.module.clone(),
                            question:question.draft.text.clone(),fingerprint:question.fingerprint.clone(),decision:answer.text.clone(),
                            accepted_by:event.actor.clone(),source:DecisionSource::AcceptedEvent{eid:event.eid.clone()},supersedes:supersedes.clone()};
                        insert(&mut self.decisions,id,decision.clone())?;
                        accepted.decision=Some(decision);
                    }
                    RememberAs::ProcessRule{id}=>{
                        valid_id(id)?;
                        insert(&mut self.rules,id,RegisteredRule{id:id.clone(),module:question.binding.module.clone(),
                            text:answer.text.clone(),registered_by:event.actor.clone(),event:event.eid.clone()})?;
                    }
                    RememberAs::SlowProposal{id}=>{
                        valid_id(id)?;
                        insert(&mut self.proposals,id,DraftProposal{id:id.clone(),source_event:event.eid.clone(),
                            text:answer.text.clone(),targets})?;
                        accepted.proposal=Some(id.clone());
                    }
                }
                insert(&mut self.answers,&answer.hold,accepted)?;
            }
            QuestionEvent::DecisionReused{question,decision}=>{
                envelope(event,"hold.decision_reused","process:towermd-dedupe",&question.draft.id)?;
                check_question(&question)?;
                check_decision(&decision,&question)?;
                if slow(&question.targets) { return Err("涉及慢层的问题不能自动应用旧回答".into()); }
                if let DecisionSource::AcceptedEvent{..}=&decision.source {
                    if self.decisions.get(&decision.id)!=Some(&decision) { return Err("去重引用不是本地已接受决策".into()); }
                }
                // MainCommit originals are read/proved by DecisionCatalog::read_main
                // before this daemon-only process event is produced.
                insert(&mut self.holds,&question.draft.id,question.clone())?;
                insert(&mut self.answers,&question.draft.id,Answer{
                    input:AnswerInput{hold:question.draft.id.clone(),text:decision.decision.clone(),
                        remember_as:RememberAs::Temporary,targets:vec![]},
                    event:event.eid.clone(),actor:event.actor.clone(),run:question.binding.run.clone(),
                    answered_at:event.t.clone(),proposal:None,decision:Some(decision),
                })?;
            }
            QuestionEvent::PromptQueued{prompt}=>{
                envelope(event,"prompt.queued","human:local",&prompt.id)?;
                let expected=prepare_prompt(prompt.id.clone(),prompt.scope.clone(),prompt.text.clone(),
                    &prompt.recipients,prompt.targets.clone())?;
                if expected!=prompt { return Err("插话收件人未按范围冻结或重复".into()); }
                if slow(&prompt.targets) {
                    let id=format!("P-{}",event.eid);
                    insert(&mut self.proposals,&id,DraftProposal{id:id.clone(),source_event:event.eid.clone(),
                        text:prompt.text,targets:prompt.targets})?;
                } else {
                    insert(&mut self.prompt_actors,&prompt.id,event.actor.clone())?;
                    insert(&mut self.prompts,&prompt.id,prompt.clone())?;
                }
            }
            QuestionEvent::Delivered{delivery}=>{
                let object=match &delivery.message {MessageId::Answer{hold,..}=>hold,MessageId::Prompt{id}=>id};
                envelope(event,"prompt.delivered","process:towermd-cli",object)?;
                valid_id(&delivery.run)?;
                let key=delivery_key(&delivery);
                let receipt=DeliveryReceipt{delivery:delivery.clone(),event:event.eid.clone(),delivered_at:event.t.clone()};
                if let Some(old)=self.deliveries.get(&key) {
                    return if old==&receipt {Ok(())} else {Err("该消息已有不同送达原件".into())};
                }
                if !self.pending_messages(&delivery.run).iter().any(|message|message.message==delivery.message) {
                    return Err("消息不存在、未生效或不属于该运行".into());
                }
                self.deliveries.insert(key,receipt);
            }
        }
        Ok(())
    }

    pub fn pending_messages(&self,run:&str)->Vec<QueuedMessage> {
        let mut queued=Vec::new();
        for (hold,answer) in &self.answers {
            if answer.run!=run || !answer.effective() {continue;}
            let message=MessageId::Answer{hold:hold.clone(),event:answer.event.clone()};
            if !self.deliveries.contains_key(&delivery_key(&Delivery{run:run.into(),message:message.clone()})) {
                queued.push(QueuedMessage{message,run:run.into(),text:answer.input.text.clone(),actor:answer.actor.clone()});
            }
        }
        for (id,prompt) in &self.prompts {
            if !prompt.recipients.iter().any(|recipient|recipient.run==run) {continue;}
            let message=MessageId::Prompt{id:id.clone()};
            if !self.deliveries.contains_key(&delivery_key(&Delivery{run:run.into(),message:message.clone()})) {
                queued.push(QueuedMessage{message,run:run.into(),text:prompt.text.clone(),
                    actor:self.prompt_actors.get(id).cloned().unwrap_or_default()});
            }
        }
        queued
    }

    pub fn wait_plan(&self,hold:&str,now:u64,requested_ms:Option<u64>)->Result<WaitPlan,String> {
        let question=self.holds.get(hold).ok_or("问题不存在")?;
        if let Some(answer)=self.answers.get(hold) {
            return if answer.effective() {Ok(WaitPlan::Answered{answer:answer.clone()})}
                else {Ok(WaitPlan::Deferred{hold:hold.into(),proposal:answer.proposal.clone()})};
        }
        if question.draft.mode==WaitMode::Parked || now>=question.deadline {
            return Ok(WaitPlan::Deferred{hold:hold.into(),proposal:None});
        }
        Ok(WaitPlan::Pending{hold:hold.into(),
            wait_ms:requested_ms.unwrap_or(ASK_WAIT_MS).min(ASK_WAIT_MS).min(question.deadline-now)})
    }
}

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(tag="status",rename_all="snake_case")]
pub enum WaitPlan {
    Answered {answer:Answer},
    Pending {hold:String,wait_ms:u64},
    Deferred {hold:String,proposal:Option<String>},
}

fn check_question(question:&Question)->Result<(),String> {
    if prepare_question(question.draft.clone(),question.binding.clone(),question.targets.clone(),question.opened_at)?!=*question {
        return Err("问题指纹或等待期限不是规范值".into());
    }
    Ok(())
}
fn check_decision(decision:&DecisionReference,question:&Question)->Result<(),String> {
    valid_id(&decision.id)?;
    nonempty(&decision.decision,"引用决策")?;
    if decision.module!=question.binding.module || decision.fingerprint!=question.fingerprint
        || fingerprint(&decision.module,&decision.question)?!=decision.fingerprint
        || !human_actor(&decision.accepted_by)
    {return Err("决策不是同模块同指纹的已接受回答".into());}
    match &decision.source {
        DecisionSource::AcceptedEvent{eid}=>crate::hub::validate_eid(eid).map_err(|e|e.to_string())?,
        DecisionSource::MainCommit{commit,path,sha256}=>{
            if !matches!(commit.len(),40|64)||!commit.bytes().all(|b|b.is_ascii_hexdigit())
                ||sha256.len()!=64||!sha256.bytes().all(|b|b.is_ascii_hexdigit()) {return Err("main决策引用不完整".into());}
            safe_path(path)?;
        }
    }
    Ok(())
}
fn envelope(event:&Event,kind:&str,actor:&str,obj:&str)->Result<(),String> {
    if event.kind!=kind||event.actor!=actor||event.obj!=obj||event.layer!="L5" {
        return Err("问答事件类型、署名或对象不匹配".into());
    }
    Ok(())
}
fn delivery_key(delivery:&Delivery)->String {
    hex::encode(Sha256::digest(serde_json::to_vec(delivery).expect("typed delivery is JSON")))
}
fn check_binding(binding:&RunContext)->Result<(),String> {
    valid_id(&binding.run)?;valid_id(&binding.task)?;valid_id(&binding.module)
}
fn check_targets(targets:&[ChangeTarget])->Result<(),String> {
    let mut ids=BTreeSet::new();
    for target in targets {
        valid_id(&target.id)?;
        if crate::types::Layer::parse(&target.layer).map(|layer|layer.as_str())!=Some(target.layer.as_str())
            ||!ids.insert(&target.id) {return Err("影响对象层级无效或重复".into());}
    }
    Ok(())
}
fn slow(targets:&[ChangeTarget])->bool {
    targets.iter().any(|target|crate::types::Layer::parse(&target.layer).is_some_and(|layer|layer.is_slow()))
}
fn valid_id(value:&str)->Result<(),String> {
    if value.is_empty()||value.starts_with('.')||value.len()>128||!value.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.')) {
        return Err("对象或运行ID无效".into());
    }
    Ok(())
}
fn nonempty(text:&str,label:&str)->Result<(),String> {
    if text.trim().is_empty()||text.len()>64*1024 {return Err(format!("{label}为空或过长"));}Ok(())
}
fn human_actor(actor:&str)->bool {
    actor.strip_prefix("human:").is_some_and(|id|!id.is_empty()&&!id.contains([':', '/'])
        && !id.chars().any(|c|c.is_whitespace()||c.is_control()))
}
fn safe_path(path:&str)->Result<(),String> {
    if path.is_empty()||path.starts_with('/')||path.contains(['\\',':'])||path.chars().any(char::is_control)
        ||path.split('/').any(|part|part.is_empty()||part=="."||part==".."||part.eq_ignore_ascii_case(".git"))
    {return Err("必须使用仓库内普通相对路径".into());}Ok(())
}
fn insert<T:PartialEq>(map:&mut BTreeMap<String,T>,id:&str,value:T)->Result<(),String> {
    match map.get(id) {
        Some(old)if old!=&value=>Err("对象ID已有不同已接受原件".into()),
        Some(_)=>Ok(()),None=>{map.insert(id.into(),value);Ok(())}
    }
}

// Unicode 17.0 general-category P ranges, frozen for stable v1 fingerprints.
// Source: https://www.unicode.org/Public/17.0.0/ucd/UnicodeData.txt
// NFKC handles compatibility punctuation first. Symbols (e.g. +, $, emoji)
// remain meaningful; punctuation inside a question is never removed.
fn punctuation(c:char)->bool {
    matches!(c as u32,
        0x21..=0x23|0x25..=0x2a|0x2c..=0x2f|0x3a..=0x3b|0x3f..=0x40|0x5b..=0x5d|
        0x5f|0x7b|0x7d|0xa1|0xa7|0xab|0xb6..=0xb7|0xbb|0xbf|0x37e|0x387|0x55a..=0x55f|
        0x589..=0x58a|0x5be|0x5c0|0x5c3|0x5c6|0x5f3..=0x5f4|0x609..=0x60a|0x60c..=0x60d|
        0x61b|0x61d..=0x61f|0x66a..=0x66d|0x6d4|0x700..=0x70d|0x7f7..=0x7f9|0x830..=0x83e|
        0x85e|0x964..=0x965|0x970|0x9fd|0xa76|0xaf0|0xc77|0xc84|0xdf4|0xe4f|0xe5a..=0xe5b|
        0xf04..=0xf12|0xf14|0xf3a..=0xf3d|0xf85|0xfd0..=0xfd4|0xfd9..=0xfda|0x104a..=0x104f|
        0x10fb|0x1360..=0x1368|0x1400|0x166e|0x169b..=0x169c|0x16eb..=0x16ed|0x1735..=0x1736|
        0x17d4..=0x17d6|0x17d8..=0x17da|0x1800..=0x180a|0x1944..=0x1945|0x1a1e..=0x1a1f|
        0x1aa0..=0x1aa6|0x1aa8..=0x1aad|0x1b4e..=0x1b4f|0x1b5a..=0x1b60|0x1b7d..=0x1b7f|
        0x1bfc..=0x1bff|0x1c3b..=0x1c3f|
        0x1c7e..=0x1c7f|0x1cc0..=0x1cc7|0x1cd3|0x2010..=0x2027|0x2030..=0x2043|
        0x2045..=0x2051|0x2053..=0x205e|0x207d..=0x207e|0x208d..=0x208e|0x2308..=0x230b|
        0x2329..=0x232a|0x2768..=0x2775|0x27c5..=0x27c6|0x27e6..=0x27ef|0x2983..=0x2998|
        0x29d8..=0x29db|0x29fc..=0x29fd|0x2cf9..=0x2cfc|0x2cfe..=0x2cff|0x2d70|
        0x2e00..=0x2e2e|0x2e30..=0x2e4f|0x2e52..=0x2e5d|0x3001..=0x3003|0x3008..=0x3011|
        0x3014..=0x301f|0x3030|0x303d|0x30a0|0x30fb|0xa4fe..=0xa4ff|0xa60d..=0xa60f|
        0xa673|0xa67e|0xa6f2..=0xa6f7|0xa874..=0xa877|0xa8ce..=0xa8cf|0xa8f8..=0xa8fa|
        0xa8fc|0xa92e..=0xa92f|0xa95f|0xa9c1..=0xa9cd|0xa9de..=0xa9df|0xaa5c..=0xaa5f|
        0xaade..=0xaadf|0xaaf0..=0xaaf1|0xabeb|0xfd3e..=0xfd3f|0xfe10..=0xfe19|
        0xfe30..=0xfe52|0xfe54..=0xfe61|0xfe63|0xfe68|0xfe6a..=0xfe6b|0xff01..=0xff03|
        0xff05..=0xff0a|0xff0c..=0xff0f|0xff1a..=0xff1b|0xff1f..=0xff20|0xff3b..=0xff3d|
        0xff3f|0xff5b|0xff5d|0xff5f..=0xff65|0x10100..=0x10102|0x1039f|0x103d0|0x1056f|
        0x10857|0x1091f|0x1093f|0x10a50..=0x10a58|0x10a7f|0x10af0..=0x10af6|
        0x10b39..=0x10b3f|0x10b99..=0x10b9c|0x10d6e|0x10ead|0x10ed0|
        0x10f55..=0x10f59|0x10f86..=0x10f89|0x11047..=0x1104d|
        0x110bb..=0x110bc|0x110be..=0x110c1|0x11140..=0x11143|0x11174..=0x11175|
        0x111c5..=0x111c8|0x111cd|0x111db|0x111dd..=0x111df|0x11238..=0x1123d|0x112a9|
        0x113d4..=0x113d5|0x113d7..=0x113d8|
        0x1144b..=0x1144f|0x1145a..=0x1145b|0x1145d|0x114c6|0x115c1..=0x115d7|
        0x11641..=0x11643|0x11660..=0x1166c|0x116b9|0x1173c..=0x1173e|0x1183b|0x11944..=0x11946|
        0x119e2|0x11a3f..=0x11a46|0x11a9a..=0x11a9c|0x11a9e..=0x11aa2|0x11b00..=0x11b09|
        0x11be1|0x11c41..=0x11c45|0x11c70..=0x11c71|0x11ef7..=0x11ef8|0x11f43..=0x11f4f|
        0x11fff|0x12470..=0x12474|0x12ff1..=0x12ff2|0x16a6e..=0x16a6f|
        0x16af5|0x16b37..=0x16b3b|0x16b44|0x16d6d..=0x16d6f|0x16e97..=0x16e9a|0x16fe2|0x1bc9f|
        0x1da87..=0x1da8b|0x1e5ff|0x1e95e..=0x1e95f)
}
