//! Read-only conversion from an actual completed initial scan to a B0 draft.
use super::{DraftBundle, DraftObject, FrozenDraft, FrozenFile};
use crate::{
    baseline_schema::{parse_submit, BaselineSubmit},
    canvas::CanvasDocument,
    catalog::Catalog,
    coldstart_evidence::CompletedColdstart,
    okf::Document,
    recovery_contract::{
        self,
        baseline::{PublicationFile, PublicationFreeze},
    },
    repo, types,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

const SLOW_TYPES: [&str; 4] = ["Intent", "Module", "Interface", "Acceptance"];

/// CompletedColdstart is obtained from the shared actual-history reader. Its scan
/// id remains the draft id, linking Evidence and candidate Tasks without publishing them.
pub fn from_scan(
    catalog: &Catalog,
    completed: &CompletedColdstart,
    submit: BaselineSubmit,
) -> Result<DraftBundle, String> {
    let submit = parse_submit(&serde_json::to_value(submit).map_err(|e| e.to_string())?)?;
    let input = completed.input();
    if submit.scan_id != completed.scan_id()
        || submit.project_id != input.project_id
        || submit.source_revision != input.revision
        || catalog.config.project_id() != input.project_id
        || catalog.common_dir != input.common_dir
    {
        return Err(
            "提交的 scan_id/project_id/source_revision 与已完成扫描或 Catalog 不一致".into(),
        );
    }
    uuid::Uuid::parse_str(&submit.project_id).map_err(|_| "project_id 必须为项目 UUID")?;
    let source = source_catalog(catalog, &submit.source_revision)?;
    initial_target(&source, "towermd/B0")?;
    let mut bundle = DraftBundle {
        id: submit.scan_id,
        project_id: submit.project_id,
        docs_dir: source.config.docs_dir().into(),
        source_revision: submit.source_revision,
        target_baseline: "towermd/B0".into(),
        objects: BTreeMap::new(),
        canvas_text: String::new(),
        deletions: vec![],
        questions: submit.questions,
        risks: submit.risks,
    };
    for (kind, objects) in [
        ("Intent", submit.intents),
        ("Module", submit.modules),
        ("Interface", submit.interfaces),
        ("Acceptance", submit.acceptances),
    ] {
        for mut object in objects {
            recovery_contract::repo_path(object.path.as_bytes())?;
            let directory = directory(kind)?;
            if !object.path.starts_with(&format!("{directory}/")) {
                return Err(format!("{kind} path 必须相对 docs_dir 位于 {directory}/"));
            }
            object.path = format!("{}/{}", bundle.docs_dir, object.path);
            let mut doc = object_document(&object, kind, &bundle.docs_dir)?;
            // Check new registered fields through the normal Document writer while
            // retaining the submitted raw UTF-8 bytes, including comments and CRLF.
            if let Some(original) = source.objects.get(&object.id) {
                // A submission may replace explicit fields/body, but omission is
                // not a request to erase a forward-compatible source field.
                for (key, value) in original.front() {
                    let key = key.as_str().ok_or("对象 frontmatter 键必须为字符串")?;
                    if doc.get(key).is_none() {
                        doc.set(key, value.clone())?;
                    }
                }
                object.text = doc.to_text();
            } else {
                let mut candidate = Document::new(kind, doc.body())?;
                for (key, value) in doc.front() {
                    candidate.set(
                        key.as_str().ok_or("对象 frontmatter 键必须为字符串")?,
                        value.clone(),
                    )?;
                }
            }
            if bundle.objects.insert(object.id.clone(), object).is_some() {
                return Err("提交对象 ID 重复".into());
            }
        }
    }
    include_retained_slow_objects(&source, &mut bundle)?;
    validate_objects(&bundle)?;
    let tree = source_tree(&source)?;
    validate_occupancy(&source, &bundle, &tree)?;
    bundle.canvas_text = draft_canvas(completed.scan(), &bundle)?;
    validate_canvas(&source, &bundle)?;
    Ok(bundle)
}

/// Open an exact current authority as a new private draft. This neither reruns
/// S6 nor publishes a baseline; only the four editable slow types enter payload.
pub fn from_current_baseline(
    catalog: &Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    id: &str,
    baseline: &str,
    main_revision: &str,
) -> Result<DraftBundle, String> {
    super::safe_id(id)?;
    recovery_contract::object_id(main_revision)?;
    let current = crate::baseline_current::current_baseline(catalog, inputs)?
        .ok_or("draft create conflict: no approved current baseline")?;
    if current.baseline != baseline || current.main_revision != main_revision {
        return Err("draft create conflict: current baseline or main revision changed".into());
    }
    let bundle = current_source_bundle(catalog, id, baseline)?;
    freeze(catalog, &bundle)?;
    Ok(bundle)
}

/// Static source reconstruction for the original Create request. This checks
/// exact imported bytes, not whether that baseline remains current or approved.
pub(crate) fn current_source_bundle(
    catalog: &Catalog,
    id: &str,
    baseline: &str,
) -> Result<DraftBundle, String> {
    super::safe_id(id)?;
    let current = crate::baseline_current::CurrentBaseline {
        baseline: baseline.into(),
        main_revision: catalog.revision.clone(),
    };
    let mut objects = BTreeMap::new();
    for (id, document) in &catalog.objects {
        if !SLOW_TYPES.contains(&document.doc_type().unwrap_or_default()) {
            continue;
        }
        let path = catalog
            .object_paths
            .get(id)
            .ok_or("current slow object lacks path")?;
        objects.insert(
            id.clone(),
            DraftObject {
                id: id.clone(),
                path: path.clone(),
                text: catalog
                    .read_file(path)?
                    .ok_or("current slow object lacks original bytes")?,
            },
        );
    }
    let bundle = DraftBundle {
        id: id.into(),
        project_id: catalog.config.project_id().into(),
        docs_dir: catalog.config.docs_dir().into(),
        source_revision: catalog.revision.clone(),
        target_baseline: crate::baseline_current::next_baseline(Some(&current))?,
        objects,
        canvas_text: catalog
            .read_file(&format!("{}/tower.canvas", catalog.config.docs_dir()))?
            .ok_or("current baseline has no canvas")?,
        deletions: vec![],
        questions: vec![],
        risks: vec![],
    };
    Ok(bundle)
}

/// Freeze exact object/canvas bytes and the complete protected source directory.
/// This computes a review subject; it does not save a ref or grant approval.
pub fn freeze(catalog: &Catalog, bundle: &DraftBundle) -> Result<FrozenDraft, String> {
    let source = source_catalog(catalog, &bundle.source_revision)?;
    if crate::baseline_current::baseline_number(&bundle.target_baseline)? == 0 {
        initial_target(catalog, &bundle.target_baseline)?;
    } else {
        later_target_source(&source, &bundle.target_baseline)?;
    }
    // The later-source gate does not certify an approved publication chain.
    // Typed producers and review additionally require actual-history proof.
    freeze_source(&source, bundle)
}

/// A newly produced save may cancel creation of objects absent from the fixed
/// source. Exact historical readers keep using `freeze`/`freeze_source` so their
/// original manifest, hash and review subject remain unchanged.
pub(crate) fn freeze_for_save(
    catalog: &Catalog,
    bundle: &DraftBundle,
) -> Result<FrozenDraft, String> {
    // Validate the complete original patch first, including canonical deletion
    // order and current protected-source equality. Normalization cannot hide an
    // invalid deletion or a slow object newly introduced by current authority.
    let mut frozen = freeze(catalog, bundle)?;
    if frozen
        .bundle
        .deletions
        .iter()
        .all(|path| frozen.base_directory.contains_key(path))
    {
        return Ok(frozen);
    }
    let mut publication = super::publication_freeze(&frozen)?;
    frozen
        .bundle
        .deletions
        .retain(|path| frozen.base_directory.contains_key(path));
    publication.deletions = frozen.bundle.deletions.clone();
    let texts = frozen
        .files
        .iter()
        .map(|(path, file)| (path.clone(), file.text.clone()))
        .collect();
    frozen.content_hash = super::publication_content_hash(&publication, &texts)?;
    super::publication_freeze(&frozen)?;
    Ok(frozen)
}

/// Static reconstruction of bytes from their fixed actual source. Historical
/// readers use this even after the target was published or current slow content
/// changed; it never certifies that a new save/review is currently valid.
pub(crate) fn freeze_source(
    catalog: &Catalog,
    bundle: &DraftBundle,
) -> Result<FrozenDraft, String> {
    if bundle.project_id != catalog.config.project_id()
        || bundle.docs_dir != catalog.config.docs_dir()
    {
        return Err("草稿项目或 docs_dir 不匹配 Catalog".into());
    }
    super::safe_id(&bundle.id)?;
    if catalog.revision != bundle.source_revision {
        return Err("静态冻结必须绑定精确来源 Catalog".into());
    }
    let source = Catalog::at_revision(&catalog.root, &bundle.source_revision)?
        .ok_or("固定来源中缺少项目配置")?;
    if source.common_dir != catalog.common_dir || !same_config(&source, catalog)? {
        return Err("静态来源配置不匹配".into());
    }
    crate::baseline_current::baseline_number(&bundle.target_baseline)?;
    // Validate the resulting source + patch view, including objects a caller
    // omitted from a manually constructed bundle. Unchanged source references
    // cannot be allowed to dangle after a deletion or endpoint edit.
    let mut effective = bundle.clone();
    include_retained_slow_objects(&source, &mut effective)?;
    validate_objects(&effective)?;
    validate_canvas(&source, &effective)?;
    let tree = source_tree(&source)?;
    validate_occupancy(&source, bundle, &tree)?;
    let base_directory =
        crate::recovery_git::publication_base_directory(&source, &source.revision)?;
    let mut files = BTreeMap::new();
    for object in bundle.objects.values() {
        let doc = Document::parse(&object.text)?;
        let proposal = new_proposal(&source, bundle, object, &doc)?;
        if !SLOW_TYPES.contains(&doc.doc_type().unwrap()) && !proposal {
            continue;
        }
        let mode = tree
            .get(&object.path)
            .map(|(mode, _)| mode.as_str())
            .unwrap_or("100644");
        files.insert(object.path.clone(), frozen_file(mode, &object.text));
    }
    let canvas_path = format!("{}/tower.canvas", bundle.docs_dir);
    let mode = tree
        .get(&canvas_path)
        .map(|(mode, _)| mode.as_str())
        .unwrap_or("100644");
    recovery_contract::file_mode(mode)?;
    files.insert(canvas_path, frozen_file(mode, &bundle.canvas_text));
    let freeze = PublicationFreeze {
        project_id: bundle.project_id.clone(),
        draft_id: bundle.id.clone(),
        docs_dir: bundle.docs_dir.clone(),
        source_revision: bundle.source_revision.clone(),
        target_baseline: bundle.target_baseline.clone(),
        base_directory: base_directory.clone(),
        files: files
            .iter()
            .map(|(path, file)| {
                (
                    path.clone(),
                    PublicationFile {
                        mode: file.mode.clone(),
                        sha256: file.sha256.clone(),
                    },
                )
            })
            .collect(),
        deletions: bundle.deletions.clone(),
        questions: bundle.questions.clone(),
        risks: bundle.risks.clone(),
    };
    let texts = files
        .iter()
        .map(|(path, file)| (path.clone(), file.text.clone()))
        .collect();
    let content_hash = super::publication_content_hash(&freeze, &texts)?;
    let frozen = FrozenDraft {
        bundle: bundle.clone(),
        files,
        base_directory,
        content_hash,
    };
    super::publication_freeze(&frozen)?;
    Ok(frozen)
}

fn source_catalog(catalog: &Catalog, revision: &str) -> Result<Catalog, String> {
    recovery_contract::object_id(revision)?;
    let source = Catalog::at_revision(&catalog.root, revision)?.ok_or("固定来源中缺少项目配置")?;
    if source.config.project_id() != catalog.config.project_id()
        || source.config.docs_dir() != catalog.config.docs_dir()
        || source.common_dir != catalog.common_dir
        || !same_config(&source, catalog)?
    {
        return Err("固定来源配置与 Catalog 不一致".into());
    }
    if crate::recovery_git::publication_base_directory(&source, revision)?
        != crate::recovery_git::publication_base_directory(catalog, &catalog.revision)?
    {
        return Err("当前受保护目录与固定草稿来源不一致".into());
    }
    Ok(source)
}
fn same_config(left: &Catalog, right: &Catalog) -> Result<bool, String> {
    let left: serde_yaml::Value =
        serde_yaml::from_str(&left.config.to_yaml()).map_err(|e| e.to_string())?;
    let right: serde_yaml::Value =
        serde_yaml::from_str(&right.config.to_yaml()).map_err(|e| e.to_string())?;
    Ok(left == right)
}
fn initial_target(catalog: &Catalog, target: &str) -> Result<(), String> {
    if target != "towermd/B0" {
        return Err("初始扫描仅能生成精确目标 towermd/B0".into());
    }
    let tags = repo::git(
        &catalog.root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/tags/towermd/B0",
        ],
    )?;
    if tags.lines().any(|tag| tag == "refs/tags/towermd/B0") {
        return Err("towermd/B0 已存在，不能重建初始基线".into());
    }
    Ok(())
}
/// Narrow dynamic guard: an initial source cannot be retargeted to Bn merely
/// by editing a string. Full approval-chain proof still requires RecoveryInputs
/// at Create/Edit/review; historical freeze_source never applies this gate.
fn later_target_source(source: &Catalog, target: &str) -> Result<(), String> {
    let number = crate::baseline_current::baseline_number(target)?;
    let prior = format!(
        "towermd/B{}",
        number.checked_sub(1).ok_or("missing prior baseline")?
    );
    let reference = format!("refs/tags/{prior}");
    let record = repo::git(
        &source.root,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(symref)",
            &reference,
        ],
    )?;
    if record != format!("{reference}\0") {
        return Err("later draft requires the exact direct preceding baseline tag".into());
    }
    let occupied = repo::git(
        &source.root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            &format!("refs/tags/{target}"),
        ],
    )?;
    if occupied
        .lines()
        .any(|name| name == format!("refs/tags/{target}"))
    {
        return Err("draft target baseline already exists".into());
    }
    let commit = repo::git(
        &source.root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )?;
    repo::git(
        &source.root,
        &["merge-base", "--is-ancestor", &commit, &source.revision],
    )?;
    let path = format!("{}/baselines/B{}.md", source.config.docs_dir(), number - 1);
    let text = source
        .read_file(&path)?
        .ok_or("later draft source lacks preceding Baseline audit")?;
    let document = Document::parse(&text)?;
    let binding = document
        .get("publication")
        .and_then(|value| value.get("binding"));
    if document.doc_type() != Some("Baseline")
        || document.get_str("id") != Some(&format!("B{}", number - 1))
        || document.get_str("git_tag") != Some(&prior)
        || binding
            .and_then(|value| value.get("target_baseline"))
            .and_then(serde_yaml::Value::as_str)
            != Some(prior.as_str())
        || binding
            .and_then(|value| value.get("project_id"))
            .and_then(serde_yaml::Value::as_str)
            != Some(source.config.project_id())
    {
        return Err("later draft source has mismatched preceding Baseline audit".into());
    }
    let tagged = Catalog::at_revision(&source.root, &commit)?
        .ok_or("preceding baseline lacks project configuration")?;
    if tagged.config.project_id() != source.config.project_id()
        || tagged.config.docs_dir() != source.config.docs_dir()
        || tagged.read_file(&path)?.as_deref() != Some(text.as_str())
    {
        return Err("preceding Baseline audit differs from its exact tagged source".into());
    }
    Ok(())
}

fn directory(kind: &str) -> Result<&'static str, String> {
    match kind {
        "Intent" => Ok("intents"),
        "Module" | "Interface" => Ok("arch"),
        "Acceptance" => Ok("specs"),
        _ => Err("对象不是可出版的慢层类型".into()),
    }
}
fn object_document(object: &DraftObject, kind: &str, docs_dir: &str) -> Result<Document, String> {
    super::safe_id(&object.id)?;
    recovery_contract::repo_path(object.path.as_bytes())?;
    if !object
        .path
        .starts_with(&format!("{docs_dir}/{}/", directory(kind)?))
        || !object.path.ends_with(".md")
    {
        return Err(format!("{} path 与 {kind} 目录不一致", object.id));
    }
    let doc = Document::parse(&object.text)?;
    let info = types::lookup(kind).ok_or("未登记对象类型")?;
    if doc.doc_type() != Some(kind) || doc.get_str("id") != Some(&object.id) {
        return Err(format!("{} 的 type/id 与提交集合不一致", object.id));
    }
    if doc.get("layer").is_some() && doc.get_str("layer") != Some(info.default_layer.as_str()) {
        return Err(format!("{} layer 与 type 不一致", object.id));
    }
    Ok(doc)
}
fn include_retained_slow_objects(source: &Catalog, bundle: &mut DraftBundle) -> Result<(), String> {
    for (id, doc) in &source.objects {
        if !SLOW_TYPES.contains(&doc.doc_type().unwrap_or_default())
            || bundle.objects.contains_key(id)
        {
            continue;
        }
        let path = source.object_paths.get(id).ok_or("来源慢层对象缺少路径")?;
        if bundle.deletions.contains(path) {
            continue;
        }
        bundle.objects.insert(
            id.clone(),
            DraftObject {
                id: id.clone(),
                path: path.clone(),
                text: doc.to_text(),
            },
        );
    }
    Ok(())
}
fn validate_objects(bundle: &DraftBundle) -> Result<(), String> {
    let mut docs = BTreeMap::new();
    let mut paths = BTreeSet::new();
    for (id, object) in &bundle.objects {
        if id != &object.id || !paths.insert(object.path.clone()) {
            return Err("对象 ID/path 重复或不一致".into());
        }
        let doc = Document::parse(&object.text)?;
        let kind = doc.doc_type().unwrap();
        if SLOW_TYPES.contains(&kind) {
            object_document(object, kind, &bundle.docs_dir)?;
        }
        docs.insert(id.as_str(), doc);
    }
    let module = |id: &str| {
        docs.get(id)
            .is_some_and(|doc| doc.doc_type() == Some("Module"))
    };
    if !docs.values().any(|doc| doc.doc_type() == Some("Module")) {
        return Err("草稿至少需要一个真实 Module".into());
    }
    for (id, doc) in &docs {
        match doc.doc_type().unwrap() {
            "Module" => {
                let paths = super::strings(doc, "paths")?;
                crate::catalog::validate_paths(&[], &paths, &bundle.docs_dir)?;
                for interface in super::strings(doc, "interfaces")? {
                    let interface = docs
                        .get(interface.as_str())
                        .ok_or("Module 引用了不存在的 Interface")?;
                    if interface.doc_type() != Some("Interface")
                        || !super::endpoints(interface)?.contains(&id.to_string())
                    {
                        return Err("Module interfaces 引用不属于自身的 Interface".into());
                    }
                }
            }
            "Interface" => {
                let ends = super::endpoints(doc)?;
                if ends[0] == ends[1] || !ends.iter().all(|id| module(id)) {
                    return Err("Interface 两端必须是不同且存在的 Module".into());
                }
            }
            "Acceptance" if !doc.get_str("module").is_some_and(module) => {
                return Err("Acceptance 引用了不存在的 Module".into());
            }
            _ => {}
        }
    }
    Ok(())
}
fn frozen_file(mode: &str, text: &str) -> FrozenFile {
    FrozenFile {
        mode: mode.into(),
        sha256: recovery_contract::sha256(text.as_bytes()),
        text: text.into(),
    }
}
type Tree = BTreeMap<String, (String, String)>;
fn source_tree(catalog: &Catalog) -> Result<Tree, String> {
    let bytes = repo::git_bytes_timeout(
        &catalog.root,
        &["ls-tree", "-rz", "--full-tree", &catalog.revision],
        Duration::from_secs(10),
    )?;
    let mut tree = BTreeMap::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let record = std::str::from_utf8(record).map_err(|_| "非 UTF-8 路径无法安全冻结")?;
        let (metadata, path) = record.split_once('\t').ok_or("无法解析来源 tree")?;
        recovery_contract::repo_path(path.as_bytes())?;
        let fields: Vec<_> = metadata.split(' ').collect();
        if fields.len() != 3 {
            return Err("无法解析来源 tree 元数据".into());
        }
        recovery_contract::object_id(fields[2])?;
        tree.insert(path.into(), (fields[0].into(), fields[2].into()));
    }
    Ok(tree)
}
fn validate_occupancy(source: &Catalog, bundle: &DraftBundle, tree: &Tree) -> Result<(), String> {
    for object in bundle.objects.values() {
        let doc = Document::parse(&object.text)?;
        let proposal = new_proposal(source, bundle, object, &doc)?;
        if !SLOW_TYPES.contains(&doc.doc_type().unwrap()) && !proposal {
            let original = source
                .read_file(&object.path)?
                .ok_or("未知对象只能原样保留已有来源")?;
            if original != object.text || source.object_paths.get(&object.id) != Some(&object.path)
            {
                return Err("不能新增或修改未知类型对象".into());
            }
            continue;
        }
        if let Some((mode, _)) = tree.get(&object.path) {
            recovery_contract::file_mode(mode)?;
            if source.object_paths.get(&object.id) != Some(&object.path)
                || source.objects.get(&object.id).and_then(Document::doc_type) != doc.doc_type()
            {
                return Err(format!(
                    "{} 已由普通文档或另一对象占用，拒绝接管",
                    object.path
                ));
            }
        } else if source.objects.contains_key(&object.id) {
            return Err("既有对象不允许隐式换路径".into());
        }
        if tree.keys().any(|path| {
            path.starts_with(&format!("{}/", object.path))
                || object.path.starts_with(&format!("{path}/"))
        }) {
            return Err("对象路径与已有目录/文件冲突".into());
        }
    }
    for path in &bundle.deletions {
        recovery_contract::repo_path(path.as_bytes())?;
        if bundle.objects.values().any(|object| &object.path == path) {
            return Err("对象路径不能同时写入和删除".into());
        }
        if let Some((mode, _)) = tree.get(path) {
            recovery_contract::file_mode(mode)?;
            let doc = source
                .object_paths
                .iter()
                .find(|(_, p)| *p == path)
                .and_then(|(id, _)| source.objects.get(id))
                .ok_or("不能删除普通文档")?;
            if !SLOW_TYPES.contains(&doc.doc_type().unwrap_or_default()) {
                return Err("不能删除非草稿慢层对象".into());
            }
        } else if !["intents", "arch", "specs"]
            .iter()
            .any(|dir| path.starts_with(&format!("{}/{dir}/", bundle.docs_dir)))
            || !path.ends_with(".md")
        {
            return Err("删除路径不在慢层对象目录".into());
        }
    }
    Ok(())
}

/// A new machine Proposal is the sole L4 payload exception. Existing opaque
/// Proposal documents keep the former byte-preserving, non-publishing path.
fn new_proposal(
    source: &Catalog,
    bundle: &DraftBundle,
    object: &DraftObject,
    doc: &Document,
) -> Result<bool, String> {
    if doc.doc_type() != Some("Proposal") || source.objects.contains_key(&object.id) {
        return Ok(false);
    }
    crate::proposals::carrier_object(object, bundle)?;
    Ok(true)
}

fn draft_canvas(scan: &Value, bundle: &DraftBundle) -> Result<String, String> {
    let text = serde_json::to_string(&scan["candidate_canvas"]).map_err(|e| e.to_string())?;
    let canvas = CanvasDocument::decode(&text)?;
    let mut raw = canvas.raw().cloned().ok_or("扫描画布无法安全编辑")?;
    let scan_modules: BTreeSet<_> = scan["modules"]
        .as_array()
        .ok_or("扫描缺少 modules")?
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    let modules: BTreeMap<_, _> = bundle
        .objects
        .iter()
        .filter(|(_, o)| Document::parse(&o.text).is_ok_and(|d| d.doc_type() == Some("Module")))
        .collect();
    let mut placed = BTreeSet::new();
    let mut removed = BTreeSet::new();
    let nodes = super::array(&mut raw, "nodes")?;
    for node in nodes.iter_mut() {
        if node["type"] != "file" {
            continue;
        }
        let Some(id) = node["towermd"]["obj"].as_str().map(str::to_owned) else {
            continue;
        };
        if let Some(object) = modules.get(&id) {
            if !placed.insert(id.clone()) {
                return Err("扫描画布模块节点重复".into());
            }
            node["file"] = json!(object.path);
        } else if scan_modules.contains(id.as_str()) {
            removed.insert(node["id"].as_str().ok_or("模块节点缺少 id")?.to_owned());
        }
    }
    nodes.retain(|node| !node["id"].as_str().is_some_and(|id| removed.contains(id)));
    for (index, (id, object)) in modules.iter().enumerate() {
        if placed.contains(id.as_str()) {
            continue;
        }
        if nodes.iter().any(|node| node["id"] == id.as_str()) {
            return Err("新 Module 与已有画布节点 ID 冲突".into());
        }
        nodes.push(json!({"id":id,"type":"file","file":object.path,"towermd":{"obj":id},"x":(index as i64)*400,"y":0,"width":320,"height":180}));
    }
    if super::array(&mut raw, "edges")?.iter().any(|edge| {
        ["fromNode", "toNode"]
            .iter()
            .any(|key| edge[*key].as_str().is_some_and(|id| removed.contains(id)))
    }) {
        return Err("替换扫描模块涉及未明确映射的画布边".into());
    }
    let encoded = serde_json::to_string(&raw).map_err(|e| e.to_string())?;
    let canvas = CanvasDocument::decode(&encoded)?;
    for object in bundle.objects.values().filter(|object| {
        Document::parse(&object.text).is_ok_and(|doc| doc.doc_type() == Some("Interface"))
    }) {
        let doc = Document::parse(&object.text)?;
        let ends = super::endpoints(&doc)?;
        let from = canvas.nodes()[super::object_node(&canvas, &bundle.objects[&ends[0]])?].raw
            ["id"]
            .as_str()
            .unwrap();
        let to = canvas.nodes()[super::object_node(&canvas, &bundle.objects[&ends[1]])?].raw["id"]
            .as_str()
            .unwrap();
        let edge_id = format!("interface:{}", object.id);
        super::vacant_id(&canvas, &edge_id)?;
        super::array(&mut raw, "edges")?.push(json!({"id":edge_id,"fromNode":from,"toNode":to,
                "label":doc.get_str("title").unwrap_or(&object.id),"towermd":{"obj":object.id}}));
    }
    serde_json::to_string_pretty(&raw).map_err(|e| e.to_string())
}
fn validate_canvas(source: &Catalog, bundle: &DraftBundle) -> Result<(), String> {
    let canvas = CanvasDocument::decode(&bundle.canvas_text)?;
    if canvas.raw().is_none() {
        return Err("画布无法安全冻结".into());
    }
    for object in bundle.objects.values() {
        let doc = Document::parse(&object.text)?;
        if doc.doc_type() == Some("Module") {
            super::object_node(&canvas, object)?;
        }
        if doc.doc_type() == Some("Interface") {
            let ends = super::endpoints(&doc)?;
            let from =
                &canvas.nodes()[super::object_node(&canvas, &bundle.objects[&ends[0]])?].raw["id"];
            let to =
                &canvas.nodes()[super::object_node(&canvas, &bundle.objects[&ends[1]])?].raw["id"];
            let edges = super::bound_edges(&canvas, &object.id)?;
            if edges.is_empty() {
                return Err("真实 Interface 缺少绑定画布边".into());
            }
            for index in edges {
                if &canvas.edges()[index].raw["fromNode"] != from
                    || &canvas.edges()[index].raw["toNode"] != to
                {
                    return Err("Interface 画布端点与对象不一致".into());
                }
            }
        }
    }
    for node in canvas
        .nodes()
        .iter()
        .filter(|node| !node.opaque && node.raw["type"] == "file")
    {
        if let Some(id) = node.raw["towermd"]["obj"].as_str() {
            if let Some(object) = bundle.objects.get(id) {
                if node.raw["file"] != object.path {
                    return Err("画布 file 与对象路径不一致".into());
                }
            } else {
                return Err("画布引用了不存在的草稿对象".into());
            }
        } else {
            super::ordinary_blob(
                &source.root,
                &bundle.source_revision,
                node.raw["file"].as_str().unwrap(),
            )?;
        }
    }
    Ok(())
}
