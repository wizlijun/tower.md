//! FR-125 commitment weakening. Every path that changes a commitment asks the
//! core's pure `weakening` comparison and, when something got looser, raises
//! one `active` `core.weakening` signal signed `process:towermd`. Raising
//! never approves, blocks or completes anything: the change itself still
//! waits for the person in the App. Each signal's hub ID derives from its
//! source, so a retry or a second host raises it once.
use super::business::now;
use super::*;
use towermd_core::{catalog::read_file_at, config::CONFIG_PATH, delegation::fingerprint, gates::GATES_PATH, question_model::SelfReported,
    hub_protocol::NewEvent, project_runtime::ProjectRuntime, tower_api, weakening::{self, FileChange, Item}};
use towermd_devkit::{dev_state::KITS, proposal_model::Change, task_model::Run};

/// Who a change is attributed to in the ledger (FR-202): the delegated
/// object and the configuration that changed it.
pub(super) struct Author<'a> {pub object:&'a str,pub source:Option<&'a SelfReported>}
impl<'a> Author<'a> {
    pub(super) fn run(run:&'a Run)->Self {Author{object:&run.task,source:run.binding.self_reported.as_ref()}}
}

/// Appends the signal for `items` unless it is already recorded or queued.
pub(super) fn raise(runtime:&mut ProjectRuntime,host:&str,project_id:&str,source:&str,target:&str,
    origin:String,author:Option<Author>,items:&[Item])->Result<(),String> {
    let eid=weakening::eid(source);
    if items.is_empty()||runtime.receipt(&eid).is_some() {return Ok(());}
    if let Some(queued)=runtime.pending().map_err(|e|e.to_string())?.into_iter().find(|event|event.eid==eid) {
        return runtime.append(queued,now).map(|_|()).map_err(|e|e.to_string());
    }
    let config=author.as_ref().map(|author|fingerprint(author.source));
    let refs=weakening::refs(origin,author.as_ref().map(|author|author.object),config.as_deref());
    let Some(envelope)=weakening::envelope(project_id,target,refs,tower_api::timestamp(now())?,items) else {return Ok(())};
    runtime.append(NewEvent{eid,host:host.into(),actor:tower_api::TOWER.into(),kind:"signal.received".into(),
        layer:tower_api::LAYER.into(),obj:target.into(),data:serde_json::to_value(envelope).map_err(|e|e.to_string())?,guards:vec![]},now)
        .map(|_|()).map_err(|e|e.to_string())
}

/// What a proposal's changes would weaken, from the bytes it carries.
pub(super) fn proposal_items(changes:&[Change])->Vec<Item> {
    let files:Vec<_>=changes.iter().map(|change|FileChange{path:&change.path,before:change.before.as_deref(),after:change.after.as_deref()}).collect();
    weakening::files(KITS,&files)
}

/// A proposal record for clients with its weakening items (`[]` when none);
/// records still waiting for their content are returned unchanged.
pub(super) fn annotate(mut record:Value)->Value {
    let Some(changes)=record.get("proposal").and_then(|proposal|proposal.get("changes")) else {return record};
    let changes:Vec<Change>=serde_json::from_value(changes.clone()).unwrap_or_default();
    record["weakening"]=json!(proposal_items(&changes));
    record
}

fn origin(id:&str)->String {format!("{}{id}",weakening::REF_PROPOSAL)}

/// Whether a proposal still owes its signal: a crash came between the two
/// appends. Only then is its run looked up again.
pub(super) fn owed(runtime:&ProjectRuntime,id:&str,changes:&[Change])->bool {
    runtime.receipt(&weakening::eid(&origin(id))).is_none() && !proposal_items(changes).is_empty()
}

/// Raised once per proposal; the target is the proposal awaiting approval.
pub(super) fn raise_for_proposal(runtime:&mut ProjectRuntime,host:&str,project_id:&str,id:&str,changes:&[Change],author:Option<Author>)->Result<(),String> {
    let origin=origin(id);
    raise(runtime,host,project_id,&origin,id,origin.clone(),author,&proposal_items(changes))
}

/// `gates.yaml` and `config.yaml` between two inspected main revisions. A
/// side that cannot be read decides nothing.
pub(super) fn raise_for_main(runtime:&mut ProjectRuntime,host:&str,root:&Path,project_id:&str,previous:&str,revision:&str)->Result<(),String> {
    if previous==revision {return Ok(());}
    let read=|revision:&str,path:&str|read_file_at(root,revision,path);
    let mut items=Vec::new();
    for (path,compare) in [(GATES_PATH,weakening::gates as fn(Option<&str>,Option<&str>)->Vec<Item>),(CONFIG_PATH,weakening::thresholds)] {
        let (Ok(before),Ok(after))=(read(previous,path),read(revision,path)) else {continue};
        items.extend(compare(before.as_deref(),after.as_deref()));
    }
    let source=format!("{previous}..{revision}");
    raise(runtime,host,project_id,&source,project_id,format!("{}{revision}",weakening::REF_MAIN),None,&items)
}
