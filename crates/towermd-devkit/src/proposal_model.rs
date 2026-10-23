//! FR-35–38 proposals reuse immutable reviewed drafts and the existing
//! publication/approval proof. A captured diff alone is never publishable.
use std::{collections::{BTreeMap,BTreeSet},path::Path,time::Duration};
use serde::{Deserialize,Serialize};
use sha2::{Digest,Sha256};
use crate::{baseline::BaselineDraft, baseline_publication::Publication, draft_store, task_model::{TaskProjection,VerifiedBaseline}};
use towermd_core::{catalog::Catalog, notemd::{self,NoteRef}, okf::Document, path_guard::{self,ChangedPath}, repo};

#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum Kind { Module,Split,Merge,Interface,Acceptance,Scope,Other }
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub draft_id:String,
    #[serde(default)]
    pub source_proposal_id:Option<String>,
    pub kind:Kind,
    pub reason:String,
    pub alternatives:Vec<String>,
    pub recommendation:String,
    /// FR-79 Note.md knowledge the proposal relies on.
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub sources:Vec<NoteRef>,
}
#[derive(Clone,Debug,Default,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct Impact {
    pub intents:Vec<String>,pub modules:Vec<String>,pub interfaces:Vec<String>,
    pub tasks:Vec<String>,pub paths:Vec<String>,
}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub path:String,
    pub before_oid:Option<String>,pub after_oid:Option<String>,
    pub before:Option<String>,pub after:Option<String>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub id:String,
    pub project_id:String,
    pub source_revision:String,
    pub baseline_tag:String,
    pub draft_id:Option<String>,
    pub source_proposal_id:Option<String>,
    pub source_commit:Option<String>,
    pub source_parent:Option<String>,
    pub kind:Kind,
    pub reason:String,
    pub alternatives:Vec<String>,
    pub recommendation:String,
    pub impact:Impact,
    pub changes:Vec<Change>,
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub sources:Vec<NoteRef>,
    pub content_hash:String,
}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct PublicationLink {
    pub id:String,
    pub content_hash:String,
    pub affected_modules:Vec<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub source_proposal_id:Option<String>,
}

fn digest(value:&impl Serialize)->Result<String,String>{
    Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(value).map_err(|e|e.to_string())?)))
}
pub fn valid_id(id:&str)->Result<(),String>{
    let value=id.strip_prefix("P-").ok_or("提案 ID 无效")?;
    if value.len()==64 && value.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)){return Ok(());}
    let uuid=uuid::Uuid::parse_str(value).map_err(|_|"提案 ID 无效")?;
    if uuid.get_version_num()!=7 || uuid.to_string()!=value{return Err("提案 ID 无效".into());}
    Ok(())
}
fn oid(value:&str)->Result<(),String>{
    if !matches!(value.len(),40|64)||!value.bytes().all(|b|b.is_ascii_hexdigit()){return Err("提案缺少固定 Git OID".into());}
    Ok(())
}
fn bytes(root:&Path,args:&[&str])->Result<Vec<u8>,String>{
    let mut options=vec!["--no-replace-objects","-c","core.hooksPath=/dev/null"];
    options.extend_from_slice(args);
    let output=repo::git_timeout_env_output(root,&options,Duration::from_secs(30),&[])?;
    if !output.status.success(){return Err(format!("提案无法完整读取 Git：{}",String::from_utf8_lossy(&output.stderr).trim()));}
    Ok(output.stdout)
}
fn blob(root:&Path,id:&str)->Result<Option<String>,String>{
    if id.bytes().all(|b|b==b'0'){return Ok(None);}
    oid(id)?;
    let size=repo::git_timeout(root,&["cat-file","-s",id],Duration::from_secs(30))?.parse::<usize>().map_err(|_|"blob 长度无效")?;
    if size>2*1024*1024{return Err("提案单个文件超过2 MiB，不能完整预览".into());}
    // Binary changes still retain exact immutable blob IDs in a capture. They
    // cannot become a publishable S6 draft through this representation.
    Ok(String::from_utf8(bytes(root,&["cat-file","blob",id])?).ok())
}
fn source_entry(catalog:&Catalog,path:&str)->Result<(Option<String>,Option<String>),String>{
    let listing=bytes(&catalog.root,&["ls-tree","-z",&catalog.revision,"--",path])?;
    if listing.is_empty(){return Ok((None,None));}
    let line=std::str::from_utf8(&listing).map_err(|_|"路径不是 UTF-8")?.trim_end_matches('\0');
    let (metadata,actual)=line.split_once('\t').ok_or("Git 路径记录无效")?;
    let fields=metadata.split_whitespace().collect::<Vec<_>>();
    if fields.len()!=3 || fields[0]!="100644" || fields[1]!="blob" || actual!=path {
        return Err("提案对象原件必须是普通文件".into());
    }
    Ok((Some(fields[2].into()),blob(&catalog.root,fields[2])?))
}
fn blob_hash(root:&Path,bytes:&[u8])->Result<String,String>{
    let output=repo::git_timeout_env_input_output(root,&["hash-object","--stdin"],Duration::from_secs(30),&[],Some(bytes))?;
    if !output.status.success(){return Err("无法计算草稿 blob 摘要".into());}
    String::from_utf8(output.stdout).map(|v|v.trim().into()).map_err(|_|"blob 摘要无效".into())
}
fn metadata_only(before:&str,after:&str)->bool{
    let (Ok(before),Ok(after))=(Document::parse(before),Document::parse(after))else{return false;};
    // Editorial provenance is regenerated when publishing the same structure.
    // Removing it here affects only impact inference, never the displayed diff.
    let mut before_front=before.front().clone();
    let mut after_front=after.front().clone();
    for key in ["status","baseline","verified","generated","sources","initial_evidence","verification"]{
        before_front.remove(&serde_yaml::Value::String(key.into()));
        after_front.remove(&serde_yaml::Value::String(key.into()));
    }
    before_front==after_front && before.body()==after.body()
}
fn endpoint_modules(document:&Document)->Vec<String>{
    ["from","to","from_module","to_module"].into_iter().filter_map(|key|document.get_str(key).map(str::to_string)).collect()
}
fn object_documents(catalog:&Catalog,draft:Option<&BaselineDraft>)->BTreeMap<String,Document>{
    let mut documents=catalog.objects.clone();
    if let Some(draft)=draft{
        for node in &draft.view.nodes{
            if let (Some(id),Ok(document))=(&node.object_id,Document::parse(&node.content)){
                documents.insert(id.clone(),document);
            }
        }
    }
    documents
}
fn infer_impact(catalog:&Catalog,draft:Option<&BaselineDraft>,changes:&[Change],tasks:&TaskProjection)->Result<Impact,String>{
    let mut modules=BTreeSet::new();let mut intents=BTreeSet::new();let mut interfaces=BTreeSet::new();
    let mut paths=BTreeSet::new();
    let documents=object_documents(catalog,draft);
    for change in changes{
        if change.before.as_deref().zip(change.after.as_deref()).is_some_and(|(a,b)|metadata_only(a,b)){continue;}
        paths.insert(change.path.clone());
        for source in [&change.before,&change.after].into_iter().flatten(){
            let Ok(document)=Document::parse(source)else{continue;};
            let Some(id)=document.get_str("id")else{continue;};
            match document.doc_type(){
                Some("Intent")=>{intents.insert(id.to_owned());}
                Some("Module")=>{
                    modules.insert(id.to_owned());
                    if let Some(value)=document.get("paths"){
                        let scope:Vec<String>=serde_yaml::from_value(value.clone()).map_err(|_|"模块路径上限无效")?;
                        for path in scope{paths.insert(path);}
                    }
                }
                Some("Acceptance")=>{
                    if let Some(module)=document.get_str("module"){modules.insert(module.into());}
                }
                Some("Interface")=>{
                    interfaces.insert(id.to_owned());
                    modules.extend(endpoint_modules(&document));
                }
                _=>{}
            }
        }
    }
    if !intents.is_empty(){
        modules.extend(documents.iter().filter(|(_,doc)|doc.doc_type()==Some("Module")).map(|(id,_)|id.clone()));
    }
    // For captured code/path violations, derive module ownership from both
    // current and proposed scope. Client-provided impact never widens access.
    for (id,document) in &documents{
        if document.doc_type()!=Some("Module"){continue;}
        let scope:Vec<String>=document.get("paths").cloned().map(serde_yaml::from_value).transpose()
            .map_err(|_|"模块路径上限无效")?.unwrap_or_default();
        if changes.iter().any(|change|scope.iter().any(|rule|
            crate::task_model::path_is_allowed(&change.path,std::slice::from_ref(rule)).unwrap_or(false))){
            modules.insert(id.clone());
        }
    }
    // An affected interface always includes both endpoints, including one
    // removed from the new draft.
    for (id,document) in &documents{
        if document.doc_type()==Some("Interface")&&interfaces.contains(id){modules.extend(endpoint_modules(document));}
    }
    for (id,document) in &documents {
        if document.doc_type()==Some("Module") && modules.contains(id) {
            let scope:Vec<String>=document.get("paths").cloned().map(serde_yaml::from_value).transpose()
                .map_err(|_|"模块路径上限无效")?.unwrap_or_default();
            paths.extend(scope);
        }
        if document.doc_type()==Some("Intent") {
            let allocated:Vec<String>=document.get("allocated_to").cloned().map(serde_yaml::from_value).transpose()
                .map_err(|_|"意图分配无效")?.unwrap_or_default();
            if allocated.iter().any(|id|modules.contains(id)){intents.insert(id.clone());}
        }
    }
    let affected_tasks=tasks.tasks.iter().filter(|(_,task)|{
        modules.contains(&task.input.module)||task.input.touches.iter().any(|id|modules.contains(id))
    }).map(|(id,_)|id.clone()).collect();
    Ok(Impact{intents:intents.into_iter().collect(),modules:modules.into_iter().collect(),
        interfaces:interfaces.into_iter().collect(),tasks:affected_tasks,paths:paths.into_iter().collect()})
}
impl Proposal{
    pub fn hash(&self)->Result<String,String>{
        // Proposals without Note.md sources keep exactly their v1 hash.
        if !self.sources.is_empty(){
            return digest(&("proposal-v2",&self.id,&self.project_id,&self.source_revision,&self.baseline_tag,
                &self.draft_id,&self.source_proposal_id,&self.source_commit,&self.source_parent,
                self.kind,&self.reason,&self.alternatives,&self.recommendation,&self.impact,&self.changes,&self.sources));
        }
        digest(&("proposal-v1",&self.id,&self.project_id,&self.source_revision,&self.baseline_tag,
            &self.draft_id,&self.source_proposal_id,&self.source_commit,&self.source_parent,
            self.kind,&self.reason,&self.alternatives,&self.recommendation,&self.impact,&self.changes))
    }
    pub fn validate(&self)->Result<(),String>{
        valid_id(&self.id)?;oid(&self.source_revision)?;
        if let Some(id)=&self.source_proposal_id{valid_id(id)?;}
        if let Some(id)=&self.draft_id{draft_store::valid_id(id)?;}
        for source in &self.sources{source.check()?;}
        if self.project_id.is_empty()||!self.baseline_tag.starts_with("towermd/B")
            ||self.reason.trim().is_empty()||self.recommendation.trim().is_empty()
            ||self.alternatives.is_empty()||self.alternatives.iter().any(|value|value.trim().is_empty())
            ||self.content_hash!=self.hash()?{
            return Err("提案理由、方案、来源或内容摘要不完整".into());
        }
        Ok(())
    }
}
fn approved_objects(catalog:&Catalog,baseline:&VerifiedBaseline)->Result<Vec<(String,String,String,Document)>,String>{
    let target=baseline.tag().strip_prefix("towermd/").ok_or("批准基线标签无效")?;
    let manifest_path=format!("{}/baselines/{target}.md",catalog.config.docs_dir());
    let manifest=bytes(&catalog.root,&["show",&format!("{}:{manifest_path}",baseline.commit())])?;
    let manifest=Document::parse(std::str::from_utf8(&manifest).map_err(|_|"批准清单不是 UTF-8")?)?;
    let objects=manifest.get("objects").and_then(serde_yaml::Value::as_mapping).ok_or("批准清单缺少对象表")?;
    let mut result=Vec::new();
    for (id,entry) in objects {
        let id=id.as_str().ok_or("清单对象 ID 无效")?;
        let path=entry.get("path").and_then(serde_yaml::Value::as_str).ok_or("清单对象路径缺失")?;
        if !path.starts_with(&format!("{}/",catalog.config.docs_dir()))
            ||path.starts_with(&format!("{}/state/",catalog.config.docs_dir()))
            ||path.starts_with(&format!("{}/baselines/",catalog.config.docs_dir()))
            ||!crate::task_model::path_is_allowed(path,&[path.into()])?{return Err("批准清单对象路径超出文档边界".into());}
        let original=bytes(&catalog.root,&["show",&format!("{}:{path}",baseline.commit())])?;
        let hash=format!("{:x}",Sha256::digest(&original));
        if entry.get("sha256").and_then(serde_yaml::Value::as_str)!=Some(hash.as_str()){return Err("批准清单对象摘要不同".into());}
        let text=String::from_utf8(original).map_err(|_|"批准对象不是 UTF-8")?;
        let document=Document::parse(&text)?;
        if document.get_str("id")!=Some(id){return Err("批准清单对象身份不同".into());}
        result.push((id.into(),path.into(),text,document));
    }
    Ok(result)
}
fn retired_objects(catalog:&Catalog,baseline:&VerifiedBaseline,files:&BTreeMap<String,Vec<u8>>)->Result<Vec<(String,String,String)>,String>{
    let mut removed=Vec::new();
    for (_,path,text,document) in approved_objects(catalog,baseline)? {
        if files.contains_key(&path)||!matches!(document.doc_type(),Some("Intent"|"Module"|"Interface"|"Acceptance")){continue;}
        let (before_oid,before)=source_entry(catalog,&path)?;
        if before.as_deref()!=Some(text.as_str()){return Err("待移除对象与批准基线不同，须先处理 main 变更".into());}
        removed.push((path,before_oid.ok_or("待移除对象不存在")?,text));
    }
    removed.sort_by(|a,b|a.0.cmp(&b.0));Ok(removed)
}
/// Preserve prior approved decisions byte for byte. Arbitrary new main
/// documents never become G0 authority through this inheritance.
pub fn inherit_decisions(publication:&mut Publication,catalog:&Catalog,baseline:&VerifiedBaseline)->Result<(),String>{
    if publication.source_revision!=catalog.revision||publication.project_id!=catalog.config.project_id(){return Err("继承决策的出版来源不同".into());}
    let manifest_path=format!("{}/baselines/{}.md",catalog.config.docs_dir(),publication.target);
    let mut manifest=Document::parse(std::str::from_utf8(publication.files.get(&manifest_path).ok_or("出版缺少清单")?).map_err(|e|e.to_string())?)?;
    let mut objects=manifest.get("objects").cloned().ok_or("清单缺少对象")?;
    let mapping=objects.as_mapping_mut().ok_or("清单对象不是映射")?;
    for (id,path,text,document) in approved_objects(catalog,baseline)? {
        if document.doc_type()!=Some("Decision"){continue;}
        if !baseline.decisions().contains(&id){return Err("旧清单决策未包含在已验证批准原件中".into());}
        if source_entry(catalog,&path)?.1.as_deref()!=Some(text.as_str()){return Err("旧批准决策在 main 上已改变，不能顺带批准".into());}
        if publication.files.contains_key(&path)||mapping.contains_key(&serde_yaml::Value::String(id.clone())){return Err("新草稿与旧批准决策身份冲突".into());}
        mapping.insert(serde_yaml::Value::String(id),serde_yaml::to_value(serde_json::json!({"path":path,"sha256":format!("{:x}",Sha256::digest(text.as_bytes()))})).map_err(|e|e.to_string())?);
        publication.files.insert(path,text.into_bytes());
    }
    manifest.set("objects",objects)?;publication.files.insert(manifest_path,manifest.to_text().into_bytes());
    publication.content_hash=publication.hash()?;Ok(())
}
pub fn prepare(
    catalog:&Catalog,draft:&BaselineDraft,tasks:&TaskProjection,baseline:&VerifiedBaseline,id:&str,input:Input,
)->Result<Proposal,String>{
    valid_id(id)?;draft_store::valid_id(&input.draft_id)?;
    if draft.id!=input.draft_id||draft.scan.project_id!=catalog.config.project_id()
        ||draft.scan.revision!=catalog.revision||!catalog.matches_current_source()?{
        return Err("提案草稿必须来自当前权威 main 的真实固定扫描".into());
    }
    let mut changes=Vec::new();
    let files=draft_store::files(draft)?;
    for (path,before_oid,before) in retired_objects(catalog,baseline,&files)? {
        changes.push(Change{path,before_oid:Some(before_oid),after_oid:None,before:Some(before),after:None});
    }
    for (path,after) in files{
        let (before_oid,before)=source_entry(catalog,&path)?;
        let after_text=String::from_utf8(after.clone()).map_err(|_|"草稿不是完整 UTF-8 文档")?;
        if before.as_deref()==Some(after_text.as_str()){continue;}
        changes.push(Change{path,before_oid,after_oid:Some(blob_hash(&catalog.root,&after)?),before,after:Some(after_text)});
    }
    let impact=infer_impact(catalog,Some(draft),&changes,tasks)?;
    let mut proposal=Proposal{
        id:id.into(),project_id:catalog.config.project_id().into(),source_revision:catalog.revision.clone(),
        baseline_tag:baseline.tag().into(),draft_id:Some(draft.id.clone()),source_proposal_id:input.source_proposal_id,
        source_commit:None,source_parent:None,kind:input.kind,reason:input.reason,
        alternatives:input.alternatives,recommendation:input.recommendation,impact,changes,sources:input.sources,content_hash:String::new(),
    };
    proposal.content_hash=proposal.hash()?;proposal.validate()?;Ok(proposal)
}

/// Capture only an actual immutable Git edge. It remains a draft until an
/// agent supplies a complete reviewed BaselineDraft through prepare().
pub fn prepare_from_changes(
    catalog:&Catalog,tasks:&TaskProjection,baseline_tag:&str,commit:&str,parent:&str,expected:&[ChangedPath],
)->Result<Proposal,String>{
    oid(commit)?;oid(parent)?;
    let parents=repo::git_timeout(&catalog.root,&["--no-replace-objects","show","-s","--format=%P",commit],Duration::from_secs(30))?;
    if !parents.split_whitespace().any(|candidate|candidate==parent){return Err("捕获提案没有真实提交父边".into());}
    let allowed=expected.iter().map(|change|change.path.clone()).collect::<Vec<_>>();
    let actual=path_guard::check(&catalog.root,parent,commit,catalog.config.docs_dir(),&allowed)?;
    if actual.changes!=expected{return Err("捕获提案与真实完整 diff 不同".into());}
    let mut changes=Vec::new();
    for change in expected{
        changes.push(Change{
            path:change.path.clone(),
            before_oid:(!change.old_oid.bytes().all(|b|b==b'0')).then(||change.old_oid.clone()),
            after_oid:(!change.new_oid.bytes().all(|b|b==b'0')).then(||change.new_oid.clone()),
            before:blob(&catalog.root,&change.old_oid)?,after:blob(&catalog.root,&change.new_oid)?,
        });
    }
    let id=format!("P-{}",digest(&(catalog.config.project_id(),&catalog.revision,commit,parent,&changes))?);
    let impact=infer_impact(catalog,None,&changes,tasks)?;
    let mut proposal=Proposal{
        id,project_id:catalog.config.project_id().into(),source_revision:catalog.revision.clone(),
        baseline_tag:baseline_tag.into(),draft_id:None,source_proposal_id:None,
        source_commit:Some(commit.into()),source_parent:Some(parent.into()),kind:Kind::Other,
        reason:"从真实越界或慢层改动捕获；尚未补全可批准草稿基线".into(),
        alternatives:vec!["撤销原改动".into(),"补全审阅草稿后申请新基线".into()],
        recommendation:"先保留原始差异，补全真实审阅和验证后再由 App 批准".into(),
        impact,changes,sources:vec![],content_hash:String::new(),
    };
    proposal.content_hash=proposal.hash()?;proposal.validate()?;Ok(proposal)
}

/// Extend the already reviewed publication with its required L1 decision in
/// the same commit. Existing publications retain exactly their old hash format.
pub fn attach_to_publication(publication:&mut Publication,proposal:&Proposal,docs:&str)->Result<(),String>{
    proposal.validate()?;
    if proposal.draft_id.as_deref()!=Some(publication.draft_id.as_str())
        ||proposal.project_id!=publication.project_id||proposal.source_revision!=publication.source_revision{
        return Err("提案与实际审阅出版输入不同".into());
    }
    publication.removals=proposal.changes.iter().filter(|change|change.after_oid.is_none())
        .map(|change|change.path.clone()).collect();
    publication.removals.sort();
    if publication.removals.iter().any(|path|publication.files.contains_key(path)){
        return Err("提案删除路径与实际草稿冲突".into());
    }
    let link=PublicationLink{id:proposal.id.clone(),content_hash:proposal.content_hash.clone(),affected_modules:proposal.impact.modules.clone(),source_proposal_id:proposal.source_proposal_id.clone()};
    let id=format!("D-{}",proposal.id);
    let path=format!("{docs}/decisions/{id}.md");
    if publication.files.contains_key(&path){return Err("提案决策路径与草稿对象冲突".into());}
    let mut decision=Document::new("Decision",&format!("{}\n\n建议：{}\n",proposal.reason,proposal.recommendation))?;
    for (key,value) in [
        ("id",serde_json::json!(id)),("layer",serde_json::json!("L1")),("status",serde_json::json!("stable")),
        ("baseline",serde_json::json!(publication.target)),("approved_by",serde_json::json!("human:local")),
        ("approved_at",serde_json::json!(publication.prepared_at)),("proposal_ref",serde_json::json!(link)),
        ("alternatives",serde_json::json!(proposal.alternatives)),("impact",serde_json::json!(proposal.impact)),
    ]{decision.set(key,serde_yaml::to_value(value).map_err(|e|e.to_string())?)?;}
    if !proposal.sources.is_empty(){
        decision.set("sources",serde_yaml::Value::Sequence(notemd::note_sources(&proposal.sources)))?;
    }
    let decision=decision.to_text().into_bytes();
    let manifest_path=format!("{docs}/baselines/{}.md",publication.target);
    let mut manifest=Document::parse(std::str::from_utf8(publication.files.get(&manifest_path).ok_or("出版缺少清单")?).map_err(|e|e.to_string())?)?;
    let mut objects=manifest.get("objects").cloned().ok_or("清单缺少对象")?;
    let mapping=objects.as_mapping_mut().ok_or("清单对象不是映射")?;
    mapping.insert(serde_yaml::Value::String(id),serde_yaml::to_value(serde_json::json!({
        "path":path,"sha256":format!("{:x}",Sha256::digest(&decision))
    })).map_err(|e|e.to_string())?);
    manifest.set("objects",objects)?;
    publication.files.insert(path,decision);
    publication.files.insert(manifest_path,manifest.to_text().into_bytes());
    publication.content_hash=publication.hash()?;
    Ok(())
}
pub fn publication_link(publication:&Publication)->Result<Option<PublicationLink>,String>{
    let mut found=None;
    for (path,bytes) in &publication.files{
        if !path.ends_with(".md"){continue;}
        let document=Document::parse(std::str::from_utf8(bytes).map_err(|e|e.to_string())?)?;
        let Some(value)=document.get("proposal_ref")else{continue;};
        if document.doc_type()!=Some("Decision")||document.get_str("layer")!=Some("L1"){
            return Err("提案出版引用只能来自对应 L1 决策".into());
        }
        // Inherited ADRs retain their original baseline and signature; only
        // the newly approved baseline's own ADR links this publication.
        if document.get_str("baseline")!=Some(publication.target.as_str()){continue;}
        let link:PublicationLink=serde_yaml::from_value(value.clone()).map_err(|e|e.to_string())?;
        valid_id(&link.id)?;
        if link.content_hash.len()!=64||!link.content_hash.bytes().all(|b|b.is_ascii_hexdigit())||document.get_str("id")!=Some(format!("D-{}",link.id).as_str())
            ||found.replace(link).is_some(){return Err("提案出版决策身份不唯一或摘要无效".into());}
    }
    Ok(found)
}

/// A question-only capture is a valid source, but never a publishable draft.
/// Its original canonical fields must match the independently typed question
/// projection before a complete reviewed proposal may refer to it.
pub fn validate_source(state:&serde_json::Value,proposal:&Proposal)->Result<Option<towermd_core::question_model::DraftProposal>,String>{
    let Some(source)=&proposal.source_proposal_id else{return Ok(None);};
    valid_id(source)?;
    let entry=state["Proposal"].get(source).ok_or("原始捕获提案不存在")?;
    if entry["status"]!="draft"{return Err("原始捕获提案已被处理，不能再次批准".into());}
    if entry.get("proposal").is_some(){
        let original=from_state(state,source)?;
        if original.draft_id.is_some()||original.project_id!=proposal.project_id{return Err("补全提案须引用同项目的原始捕获草稿".into());}
        return Ok(None);
    }
    let questions=towermd_core::question_projection::read(state)?;
    let original=questions.proposals.get(source).ok_or("来源不是已接受的问答提案原件")?;
    if entry["source_event"]!=original.source_event||entry["text"]!=original.text
        ||entry["targets"]!=serde_json::to_value(&original.targets).map_err(|e|e.to_string())?{
        return Err("问答提案原件与展示投影不一致".into());
    }
    Ok(Some(original.clone()))
}

/// Only daemon-produced immutable proposal records are accepted. Question-only
/// drafts use a different shape and cannot be treated as reviewed publications.
pub fn from_state(state:&serde_json::Value,id:&str)->Result<Proposal,String>{
    valid_id(id)?;
    let entry=state["Proposal"].get(id).ok_or("提案不存在")?;
    let proposal:Proposal=serde_json::from_value(entry["proposal"].clone()).map_err(|_|"该草稿尚未补全真实审阅基线")?;
    proposal.validate()?;
    if proposal.id!=id{return Err("提案身份不同".into());}
    Ok(proposal)
}
pub fn apply(state:&mut serde_json::Value,event:&towermd_core::hub::Event)->Result<bool,String>{
    use serde_json::json;
    match event.kind.as_str(){
        "proposal.submitted"|"proposal.captured"=>{
            let proposal:Proposal=serde_json::from_value(event.data.clone()).map_err(|e|e.to_string())?;
            proposal.validate()?;
            let submitted=event.kind=="proposal.submitted";
            if event.layer!="L4"||event.obj!=proposal.id||!event.guards.is_empty()
                ||(submitted&&(event.actor!="process:towermd-cli"||proposal.draft_id.is_none()
                    ||proposal.id!=format!("P-{}",event.eid)))
                ||(!submitted&&(event.actor!="process:towermd"||proposal.draft_id.is_some()
                    ||proposal.source_commit.is_none()||proposal.source_parent.is_none())){
                return Err("提案须由可信流程提交完整固定内容，不能直接批准".into());
            }
            validate_source(state,&proposal)?;
            let entry=json!({"type":"Proposal","id":proposal.id,"status":if submitted{"submitted"}else{"draft"},
                "event":event.eid,"proposal":proposal});
            let group=state["Proposal"].as_object_mut().ok_or("提案投影无效")?;
            if let Some(old)=group.get(&event.obj){if old!=&entry{return Err("提案 ID 已存在且内容不同".into());}}
            else{group.insert(event.obj.clone(),entry);}
        }
        "proposal.rejected"=>{
            let proposal=from_state(state,&event.obj)?;
            if event.actor!="human:local"||event.layer!="L4"||!event.guards.is_empty()
                ||event.data["proposal_hash"].as_str()!=Some(proposal.content_hash.as_str())
                ||event.data["reason"].as_str().is_none_or(|value|value.trim().is_empty()){
                return Err("驳回须来自 App 的明确人工动作并绑定原提案".into());
            }
            let entry=&mut state["Proposal"][&event.obj];
            if entry["status"]=="approved"{return Err("已出版提案不能驳回".into());}
            entry["status"]=json!("rejected");entry["rejection"]=json!({"eid":event.eid,"reason":event.data["reason"]});
        }
        _=>return Ok(false),
    }
    Ok(true)
}

#[cfg(test)]
#[path="proposal_model_tests.rs"]
mod tests;
