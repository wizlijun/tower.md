//! Shared persistent data. These DTOs describe facts; they do not confer approval or trust.
use serde::{Deserialize, Serialize};

pub mod verification {
    use super::*;
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct VerificationCommand {
        pub cwd: std::path::PathBuf,
        pub argv: Vec<String>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct VerificationReport {
        pub commit: String,
        pub tree: String,
        pub executions: Vec<CommandExecution>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct CommandExecution {
        pub command: VerificationCommand,
        pub result: String,
        #[serde(deserialize_with = "super::required_nullable")]
        pub exit_code: Option<i32>,
        pub timed_out: bool,
        pub stdout_log: String,
        pub stderr_log: String,
        pub stdout_sha256: String,
        pub stderr_sha256: String,
        #[serde(deserialize_with = "super::required_nullable")]
        pub diagnostic: Option<String>,
    }
    pub fn commands_hash(commands: &[VerificationCommand]) -> Result<String, String> {
        super::canonical_hash(commands)
    }
    pub fn report_hash(report: &VerificationReport) -> Result<String, String> {
        super::canonical_hash(report)
    }
}
pub mod finish {
    use super::*;
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    pub enum EvidenceResult {
        Passed,
        Failed,
        NoTests,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct FrozenFinish {
        pub evidence_id: String,
        pub project_id: String,
        pub host: String,
        pub root: std::path::PathBuf,
        pub common_dir: std::path::PathBuf,
        pub task_id: String,
        pub run_id: String,
        pub module_id: String,
        pub commit: String,
        pub tree: String,
        pub base: String,
        pub authority: String,
        pub baseline: String,
        #[serde(deserialize_with = "super::required_nullable")]
        pub plan_hash: Option<String>,
        pub commands: Vec<super::verification::VerificationCommand>,
        pub commands_hash: String,
        #[serde(deserialize_with = "super::deserialize_fences")]
        pub fences: Vec<crate::hub_protocol::Fence>,
        pub entered_verifying_at: u64,
        pub merge_deadline: u64,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct VerificationLogRef {
        pub report_eid: String,
        pub report_sha256: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    pub struct FinishEvidence {
        pub id: String,
        pub kind: String,
        pub task: String,
        pub run: String,
        pub module: String,
        pub commit: String,
        pub tree: String,
        pub base: String,
        pub authority: String,
        pub baseline: String,
        #[serde(deserialize_with = "super::required_nullable")]
        pub plan_hash: Option<String>,
        pub commands_hash: String,
        #[serde(deserialize_with = "super::deserialize_fences")]
        pub fences: Vec<crate::hub_protocol::Fence>,
        pub result: EvidenceResult,
        pub generated_by: String,
        pub log: VerificationLogRef,
        pub gates: std::collections::BTreeMap<String, bool>,
        #[serde(flatten, serialize_with = "super::finish_extensions")]
        pub extra: std::collections::BTreeMap<String, serde_json::Value>,
    }
}
pub mod task_review {
    use super::*;
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    pub enum ReviewSubject {
        Plan { sha256: String },
        Tree { tree: String },
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct ReviewBinding {
        pub run: String,
        pub task: String,
        pub baseline: String,
        #[serde(deserialize_with = "super::deserialize_fences")]
        pub fences: Vec<crate::hub_protocol::Fence>,
        pub subject: ReviewSubject,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    pub enum ReviewVerdict {
        Approved,
        ChangesRequested,
        Rejected,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct ReviewFact {
        pub binding: ReviewBinding,
        pub verdict: ReviewVerdict,
        pub actor: String,
        pub answer_eid: String,
    }
}
pub mod baseline {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct DraftObject {
        pub id: String,
        pub path: String,
        pub text: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct DraftBundle {
        pub id: String,
        pub project_id: String,
        pub docs_dir: String,
        pub source_revision: String,
        pub target_baseline: String,
        pub objects: BTreeMap<String, DraftObject>,
        pub canvas_text: String,
        pub deletions: Vec<String>,
        pub questions: Vec<String>,
        pub risks: Vec<String>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct FrozenFile {
        pub mode: String,
        pub sha256: String,
        pub text: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct FrozenDraft {
        pub bundle: DraftBundle,
        pub files: BTreeMap<String, FrozenFile>,
        pub base_directory: BTreeMap<String, String>,
        pub content_hash: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct ReviewBinding {
        pub project_id: String,
        pub object_id: String,
        pub content_hash: String,
        pub target_baseline: String,
        pub draft_revision: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    pub enum Verdict {
        Approved,
        Rejected,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct ApprovalFact {
        pub approval_eid: String,
        pub binding: ReviewBinding,
        pub actor: String,
        pub verdict: Verdict,
        pub comment: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct PublicationFile {
        pub mode: String,
        pub sha256: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct PublicationFreeze {
        pub project_id: String,
        pub draft_id: String,
        pub docs_dir: String,
        pub source_revision: String,
        pub target_baseline: String,
        pub base_directory: BTreeMap<String, String>,
        pub files: BTreeMap<String, PublicationFile>,
        pub deletions: Vec<String>,
        pub questions: Vec<String>,
        pub risks: Vec<String>,
    }
    pub fn publication_freeze(draft: &FrozenDraft) -> Result<PublicationFreeze, String> {
        let b = &draft.bundle;
        let freeze = PublicationFreeze {
            project_id: b.project_id.clone(),
            draft_id: b.id.clone(),
            docs_dir: b.docs_dir.clone(),
            source_revision: b.source_revision.clone(),
            target_baseline: b.target_baseline.clone(),
            base_directory: draft.base_directory.clone(),
            files: draft
                .files
                .iter()
                .map(|(p, f)| {
                    (
                        p.clone(),
                        PublicationFile {
                            mode: f.mode.clone(),
                            sha256: f.sha256.clone(),
                        },
                    )
                })
                .collect(),
            deletions: b.deletions.clone(),
            questions: b.questions.clone(),
            risks: b.risks.clone(),
        };
        let texts = draft
            .files
            .iter()
            .map(|(p, f)| (p.clone(), f.text.clone()))
            .collect();
        if publication_content_hash(&freeze, &texts)? != draft.content_hash {
            return Err("frozen draft content_hash mismatch".into());
        }
        Ok(freeze)
    }
    pub fn publication_content_hash(
        freeze: &PublicationFreeze,
        texts: &BTreeMap<String, String>,
    ) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        super::repo_path(freeze.docs_dir.as_bytes())?;
        super::object_id(&freeze.source_revision)?;
        if freeze.project_id.is_empty()
            || freeze.draft_id.is_empty()
            || freeze.target_baseline.is_empty()
        {
            return Err("missing freeze identity".into());
        }
        if !freeze.files.keys().eq(texts.keys()) {
            return Err("publication texts differ from frozen file set".into());
        }
        let mut base = Vec::new();
        for (path, mode_oid) in &freeze.base_directory {
            super::repo_path(path.as_bytes())?;
            let (mode, oid) = mode_oid
                .split_once(' ')
                .ok_or("invalid base directory mode/OID")?;
            super::file_mode(mode)?;
            super::object_id(oid)?;
            base.push((path, mode_oid));
        }
        let mut files = Vec::new();
        for (path, file) in &freeze.files {
            super::repo_path(path.as_bytes())?;
            super::file_mode(&file.mode)?;
            super::sha256_hex(&file.sha256)?;
            if super::sha256(texts[path].as_bytes()) != file.sha256 {
                return Err(format!("publication text sha256 mismatch: {path}"));
            }
            files.push((path, &file.mode, &texts[path]));
        }
        for path in &freeze.deletions {
            super::repo_path(path.as_bytes())?;
            if freeze.files.contains_key(path) {
                return Err("publication path both written and deleted".into());
            }
        }
        if freeze.deletions.windows(2).any(|p| p[0] >= p[1]) {
            return Err("deletions must be sorted and unique".into());
        }
        let parts = [
            freeze.project_id.as_bytes().to_vec(),
            freeze.draft_id.as_bytes().to_vec(),
            freeze.docs_dir.as_bytes().to_vec(),
            freeze.source_revision.as_bytes().to_vec(),
            freeze.target_baseline.as_bytes().to_vec(),
            super::canonical_json_bytes(&base)?,
            super::canonical_json_bytes(&files)?,
            super::canonical_json_bytes(&freeze.deletions)?,
            super::canonical_json_bytes(&freeze.questions)?,
            super::canonical_json_bytes(&freeze.risks)?,
        ];
        let mut hash = Sha256::new();
        hash.update(b"towermd:baseline-freeze:v1\0");
        for part in parts {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        Ok(hex::encode(hash.finalize()))
    }
}
pub mod recorded {
    use super::*;
    use super::{baseline, task_review};
    use std::collections::BTreeMap;
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct RequestMeta {
        pub request_id: String,
        pub body_hash: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct RequestReceipt<T> {
        pub request_id: String,
        pub body_hash: String,
        pub result_ref: T,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct RequestBody<A> {
        pub project_id: String,
        pub actor: String,
        pub host: String,
        #[serde(deserialize_with = "super::required_nullable")]
        pub run: Option<String>,
        pub action: A,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    pub enum TaskReviewAction {
        Review {
            hold: String,
            binding: task_review::ReviewBinding,
            verdict: task_review::ReviewVerdict,
            comment: String,
        },
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    pub enum BaselineReviewAction {
        ReviewBaseline {
            binding: baseline::ReviewBinding,
            verdict: baseline::Verdict,
            comment: String,
        },
    }
    /// Pure shared envelopes parameterized by the one S6/S7 wire schema. The
    /// production alias fixes S/E; no generic event producer accepts this type.
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    pub enum DraftSaveAction<S, E> {
        CreateDraft {
            baseline: String,
            main_revision: String,
        },
        SubmitBaseline {
            submit: S,
        },
        EditDraft {
            id: String,
            expected_revision: String,
            edit: E,
        },
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
    pub enum DraftDelivery {
        LocalSaved,
        RemoteSaved,
        LocalSavedRemotePending { reason: String },
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct DraftSavedResult {
        pub draft_id: String,
        pub revision: String,
        pub reference: String,
        pub content_hash: String,
        pub delivery: DraftDelivery,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct DraftSavedRecorded<S, E> {
        pub result: DraftSavedResult,
        pub request_body: RequestBody<DraftSaveAction<S, E>>,
        pub p3_request: RequestMeta,
        pub p3_receipt: RequestReceipt<DraftSavedResult>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    pub enum PublicationPrepareAction {
        PreparePublication {
            publication_id: String,
            approval_eid: String,
            binding: baseline::ReviewBinding,
            authority_revision: String,
            content_hash: String,
        },
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    pub enum BaselineCreatedAction {
        RecordBaseline { result: BaselineCreatedResult },
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct TaskReviewResult {
        pub hold: String,
        pub fact: task_review::ReviewFact,
        pub verdict: task_review::ReviewVerdict,
        pub comment: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct PublicationPreparedResult {
        pub prepared_eid: String,
        pub publication_id: String,
        pub authority_revision: String,
        pub binding: baseline::ReviewBinding,
        pub content_hash: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct BaselineCreatedResult {
        pub commit: String,
        pub publication_id: String,
        pub approval_eid: String,
        pub content_hash: String,
        pub target: String,
        pub tag: String,
        pub tags: Vec<String>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    pub struct ReviewHoldView {
        #[serde(rename = "type")]
        pub object_type: String,
        pub id: String,
        pub body: ReviewBodyView,
        pub opened_ms: u64,
        pub opened_seq: u64,
        #[serde(deserialize_with = "super::required_nullable")]
        pub reply: Option<ReviewReplyView>,
        #[serde(flatten, serialize_with = "super::hold_extensions")]
        pub extra: BTreeMap<String, serde_json::Value>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    pub enum ReviewBodyView {
        Review {
            binding: task_review::ReviewBinding,
            verdict: Option<task_review::ReviewVerdict>,
            plan_content: Option<String>,
            source_commit: Option<String>,
            source_path: Option<String>,
            comment: Option<String>,
        },
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    pub struct ReviewReplyView {
        pub id: String,
        pub text: String,
        pub actor: String,
        pub at_ms: u64,
        #[serde(flatten, serialize_with = "super::reply_extensions")]
        pub extra: BTreeMap<String, serde_json::Value>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct TaskReviewRecorded {
        pub hold: ReviewHoldView,
        pub fact: task_review::ReviewFact,
        pub comment: String,
        pub request_body: RequestBody<TaskReviewAction>,
        pub p3_request: RequestMeta,
        pub p3_receipt: RequestReceipt<TaskReviewResult>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct BaselineReviewRecorded {
        pub approval: baseline::ApprovalFact,
        pub request_body: RequestBody<BaselineReviewAction>,
        pub p3_request: RequestMeta,
        pub p3_receipt: RequestReceipt<baseline::ApprovalFact>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct PublicationPrepared {
        pub publication_id: String,
        pub approval_eid: String,
        pub binding: baseline::ReviewBinding,
        pub authority_revision: String,
        pub content_hash: String,
        pub request_body: RequestBody<PublicationPrepareAction>,
        pub p3_request: RequestMeta,
        pub p3_receipt: RequestReceipt<PublicationPreparedResult>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(from = "BaselineCreatedWire")]
    pub struct BaselineCreated {
        #[serde(flatten)]
        pub result: BaselineCreatedResult,
        pub request_body: RequestBody<BaselineCreatedAction>,
        pub p3_request: RequestMeta,
        pub p3_receipt: RequestReceipt<BaselineCreatedResult>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct BaselineCreatedWire {
        commit: String,
        publication_id: String,
        approval_eid: String,
        content_hash: String,
        target: String,
        tag: String,
        tags: Vec<String>,
        request_body: RequestBody<BaselineCreatedAction>,
        p3_request: RequestMeta,
        p3_receipt: RequestReceipt<BaselineCreatedResult>,
    }
    impl From<BaselineCreatedWire> for BaselineCreated {
        fn from(v: BaselineCreatedWire) -> Self {
            Self {
                result: BaselineCreatedResult {
                    commit: v.commit,
                    publication_id: v.publication_id,
                    approval_eid: v.approval_eid,
                    content_hash: v.content_hash,
                    target: v.target,
                    tag: v.tag,
                    tags: v.tags,
                },
                request_body: v.request_body,
                p3_request: v.p3_request,
                p3_receipt: v.p3_receipt,
            }
        }
    }
    pub fn request_event_eid(request_id: &str, kind: &str) -> Result<String, String> {
        let id = uuid::Uuid::parse_str(request_id).map_err(|e| e.to_string())?;
        if id.get_version_num() != 7
            || id.get_variant() != uuid::Variant::RFC4122
            || id.hyphenated().to_string() != request_id
        {
            return Err("request_id must be canonical UUIDv7".into());
        }
        crate::events::derived_eid(&super::sha256(request_id.as_bytes()), kind)
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    pub enum ReviewGate {
        G1,
        G4,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct ReviewOpenView {
        pub hold: ReviewHoldView,
        pub gate: ReviewGate,
        pub binding: task_review::ReviewBinding,
        #[serde(deserialize_with = "super::required_nullable")]
        pub plan_content: Option<String>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct GateGenerated {
        pub by: String,
        pub at: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    pub enum GateResult {
        Passed,
        Failed,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct GateReviewLink {
        #[serde(deserialize_with = "super::deserialize_event")]
        pub opening: crate::hub_protocol::Event,
        #[serde(deserialize_with = "super::deserialize_event")]
        pub reviewed: crate::hub_protocol::Event,
        #[serde(deserialize_with = "super::required_nullable")]
        pub completed_eid: Option<String>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    pub struct GateEvidence {
        #[serde(rename = "type")]
        pub object_type: String,
        pub id: String,
        pub kind: String,
        pub gate: crate::task_gates::Gate,
        pub project_id: String,
        pub task: String,
        pub run: String,
        pub module: String,
        #[serde(deserialize_with = "super::required_nullable")]
        pub commit: Option<String>,
        #[serde(deserialize_with = "super::required_nullable")]
        pub tree: Option<String>,
        #[serde(deserialize_with = "super::required_nullable")]
        pub base: Option<String>,
        #[serde(deserialize_with = "super::required_nullable")]
        pub authority: Option<String>,
        #[serde(deserialize_with = "super::required_nullable")]
        pub baseline: Option<String>,
        #[serde(deserialize_with = "super::required_nullable")]
        pub plan_hash: Option<String>,
        #[serde(deserialize_with = "super::required_nullable")]
        pub plan_opening_eid: Option<String>,
        #[serde(deserialize_with = "super::required_nullable")]
        pub commands_hash: Option<String>,
        #[serde(deserialize_with = "super::deserialize_fences")]
        pub fences: Vec<crate::hub_protocol::Fence>,
        pub result: GateResult,
        #[serde(deserialize_with = "super::required_nullable")]
        pub review: Option<GateReviewLink>,
        #[serde(deserialize_with = "super::required_nullable")]
        pub diagnostic: Option<String>,
        pub generated: GateGenerated,
        #[serde(flatten, serialize_with = "super::gate_extensions")]
        pub extra: BTreeMap<String, serde_json::Value>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct ReviewContext {
        pub gate: ReviewGate,
        #[serde(deserialize_with = "super::deserialize_event")]
        pub opening: crate::hub_protocol::Event,
        #[serde(deserialize_with = "super::deserialize_event")]
        pub reviewed: crate::hub_protocol::Event,
        #[serde(deserialize_with = "super::deserialize_event")]
        pub gate_evidence: crate::hub_protocol::Event,
    }
    /// Original terminal G5 capture; a layout view, never an authority constructor.
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    pub struct IntegrationGateRecorded {
        pub evidence: GateEvidence,
        pub completed_eid: String,
        pub parents: Vec<String>,
    }
}

// Shared only inside core: callers cannot provide a canonicalized trusted state.
pub(crate) fn canonical_json_bytes<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, String> {
    fn sorted(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut entries = map.into_iter().collect::<Vec<_>>();
                entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
                serde_json::Value::Object(
                    entries.into_iter().map(|(k, v)| (k, sorted(v))).collect(),
                )
            }
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(sorted).collect())
            }
            other => other,
        }
    }
    serde_json::to_vec(&sorted(
        serde_json::to_value(value).map_err(|e| e.to_string())?,
    ))
    .map_err(|e| e.to_string())
}
pub(crate) fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}
pub(crate) fn canonical_hash<T: Serialize + ?Sized>(value: &T) -> Result<String, String> {
    Ok(sha256(&canonical_json_bytes(value)?))
}
pub(crate) fn sha256_hex(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid lowercase SHA256".into());
    }
    Ok(())
}
pub(crate) fn object_id(value: &str) -> Result<(), String> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid full Git object ID".into());
    }
    Ok(())
}
pub(crate) fn file_mode(value: &str) -> Result<(), String> {
    if !matches!(value, "100644" | "100755") {
        return Err("unsupported file mode".into());
    }
    Ok(())
}
pub(crate) fn repo_path(path: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(path).map_err(|_| "non-UTF8 repository path")?;
    if text.is_empty()
        || text.starts_with('/')
        || text.contains(['\\', '\0'])
        || text
            .split('/')
            .any(|p| p.is_empty() || matches!(p, "." | ".." | ".git") || p.contains(':'))
    {
        return Err("invalid repository-relative path".into());
    }
    Ok(())
}

pub(crate) fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

// Pure binding validation, deliberately not an accepted-history constructor.
#[allow(
    dead_code,
    reason = "E1 and P2 will consume the shared completion binding validator"
)]
pub(crate) fn validate_finish_report(
    input: &finish::FrozenFinish,
    completed_eid: &str,
    evidence: &finish::FinishEvidence,
    report: &verification::VerificationReport,
) -> Result<(), String> {
    use finish::EvidenceResult;
    if evidence.id != input.evidence_id
        || evidence.task != input.task_id
        || evidence.run != input.run_id
        || evidence.module != input.module_id
        || evidence.commit != input.commit
        || evidence.tree != input.tree
        || evidence.base != input.base
        || evidence.authority != input.authority
        || evidence.baseline != input.baseline
        || evidence.plan_hash != input.plan_hash
        || evidence.commands_hash != input.commands_hash
        || evidence.fences != input.fences
        || evidence.generated_by != "process:towermd-verifier"
    {
        return Err("completion evidence differs from frozen input".into());
    }
    if report.commit != input.commit
        || report.tree != input.tree
        || verification::commands_hash(&input.commands)? != input.commands_hash
        || report
            .executions
            .iter()
            .map(|e| &e.command)
            .ne(input.commands.iter())
    {
        return Err("completion report differs from frozen commit/tree/commands".into());
    }
    if evidence.log.report_eid != completed_eid
        || evidence.log.report_sha256 != verification::report_hash(report)?
    {
        return Err("completion report locator/hash mismatch".into());
    }
    for execution in &report.executions {
        sha256_hex(&execution.stdout_sha256)?;
        sha256_hex(&execution.stderr_sha256)?;
        if execution.stdout_log.is_empty() || execution.stderr_log.is_empty() {
            return Err("missing immutable execution log locator".into());
        }
    }
    let passed = !report.executions.is_empty()
        && report
            .executions
            .iter()
            .all(|e| e.result == "passed" && e.exit_code == Some(0) && !e.timed_out);
    match evidence.result {
        EvidenceResult::Passed if !passed => {
            Err("passed evidence lacks successful execution records".into())
        }
        EvidenceResult::NoTests if !report.executions.is_empty() || !input.commands.is_empty() => {
            Err("no_tests has execution/command records".into())
        }
        EvidenceResult::Failed if passed => {
            Err("failed evidence contradicts successful report".into())
        }
        _ => {
            canonical_json_bytes(evidence)?;
            Ok(())
        }
    }
}

fn finish_extensions<S: serde::Serializer>(
    extra: &std::collections::BTreeMap<String, serde_json::Value>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if extra.keys().any(|key| {
        [
            "id",
            "kind",
            "task",
            "run",
            "module",
            "commit",
            "tree",
            "base",
            "authority",
            "baseline",
            "plan_hash",
            "commands_hash",
            "fences",
            "result",
            "generated_by",
            "log",
            "gates",
        ]
        .contains(&key.as_str())
    }) {
        return Err(serde::ser::Error::custom(
            "extension overrides a fixed field",
        ));
    }
    extra.serialize(serializer)
}

fn hold_extensions<S: serde::Serializer>(
    extra: &std::collections::BTreeMap<String, serde_json::Value>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if extra.keys().any(|key| {
        ["type", "id", "body", "opened_ms", "opened_seq", "reply"].contains(&key.as_str())
    }) {
        return Err(serde::ser::Error::custom(
            "extension overrides a fixed field",
        ));
    }
    extra.serialize(serializer)
}

fn reply_extensions<S: serde::Serializer>(
    extra: &std::collections::BTreeMap<String, serde_json::Value>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if extra
        .keys()
        .any(|key| ["id", "text", "actor", "at_ms"].contains(&key.as_str()))
    {
        return Err(serde::ser::Error::custom(
            "extension overrides a fixed field",
        ));
    }
    extra.serialize(serializer)
}

fn gate_extensions<S: serde::Serializer>(
    extra: &std::collections::BTreeMap<String, serde_json::Value>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if extra.keys().any(|key| {
        [
            "type",
            "id",
            "kind",
            "gate",
            "project_id",
            "task",
            "run",
            "module",
            "commit",
            "tree",
            "base",
            "authority",
            "baseline",
            "plan_hash",
            "plan_opening_eid",
            "commands_hash",
            "fences",
            "result",
            "review",
            "diagnostic",
            "generated",
        ]
        .contains(&key.as_str())
    }) {
        return Err(serde::ser::Error::custom(
            "extension overrides a fixed field",
        ));
    }
    extra.serialize(serializer)
}

#[allow(
    dead_code,
    reason = "E1/P3 reuse the shared recorded layout validators"
)]
pub(crate) fn validate_recorded_data(
    project: &str,
    kind: &str,
    data: &serde_json::Value,
) -> Result<(), String> {
    use recorded::*;
    match kind {
        "hold.reviewed" => {
            let v: TaskReviewRecorded =
                serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
            let TaskReviewAction::Review {
                hold,
                binding,
                verdict,
                comment,
            } = &v.request_body.action;
            let ReviewBodyView::Review {
                binding: hold_binding,
                verdict: hold_verdict,
                comment: hold_comment,
                ..
            } = &v.hold.body;
            let reply = v
                .hold
                .reply
                .as_ref()
                .ok_or("review has no original reply")?;
            if v.hold.object_type != "Hold"
                || hold != &v.hold.id
                || binding != &v.fact.binding
                || hold_binding != binding
                || verdict != &v.fact.verdict
                || hold_verdict.as_ref() != Some(verdict)
                || comment != &v.comment
                || hold_comment.as_ref() != Some(comment)
                || reply.text != *comment
                || reply.actor != v.fact.actor
                || v.request_body.actor != v.fact.actor
                || v.fact.answer_eid != request_event_eid(&v.p3_request.request_id, kind)?
            {
                return Err("review request/fact/Hold binding mismatch".into());
            }
            human_actor(&v.fact.actor)?;
            let result = TaskReviewResult {
                hold: v.hold.id.clone(),
                fact: v.fact.clone(),
                verdict: v.fact.verdict.clone(),
                comment: v.comment.clone(),
            };
            check_request(
                project,
                &v.request_body,
                &v.p3_request,
                &v.p3_receipt,
                &result,
            )
        }
        "baseline.approved" | "baseline.rejected" => {
            let v: BaselineReviewRecorded =
                serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
            let BaselineReviewAction::ReviewBaseline {
                binding,
                verdict,
                comment,
            } = &v.request_body.action;
            let expected_kind = match v.approval.verdict {
                baseline::Verdict::Approved => "baseline.approved",
                baseline::Verdict::Rejected => "baseline.rejected",
            };
            if kind != expected_kind
                || v.approval.binding.project_id != project
                || binding != &v.approval.binding
                || verdict != &v.approval.verdict
                || comment != &v.approval.comment
                || v.request_body.actor != v.approval.actor
                || v.approval.approval_eid != request_event_eid(&v.p3_request.request_id, kind)?
            {
                return Err("baseline review request/fact mismatch".into());
            }
            human_actor(&v.approval.actor)?;
            check_request(
                project,
                &v.request_body,
                &v.p3_request,
                &v.p3_receipt,
                &v.approval,
            )
        }
        "baseline.publication_prepared" => {
            let v: PublicationPrepared =
                serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
            let PublicationPrepareAction::PreparePublication {
                publication_id,
                approval_eid,
                binding,
                authority_revision,
                content_hash,
            } = &v.request_body.action;
            if v.binding.project_id != project
                || v.publication_id != v.approval_eid
                || publication_id != &v.publication_id
                || approval_eid != &v.approval_eid
                || binding != &v.binding
                || authority_revision != &v.authority_revision
                || content_hash != &v.content_hash
                || v.binding.content_hash != v.content_hash
                || v.request_body.actor != "process:towermd-baseline"
            {
                return Err("publication preparation request/fact mismatch".into());
            }
            object_id(&v.authority_revision)?;
            sha256_hex(&v.content_hash)?;
            let result = PublicationPreparedResult {
                prepared_eid: request_event_eid(&v.p3_request.request_id, kind)?,
                publication_id: v.publication_id.clone(),
                authority_revision: v.authority_revision.clone(),
                binding: v.binding.clone(),
                content_hash: v.content_hash.clone(),
            };
            check_request(
                project,
                &v.request_body,
                &v.p3_request,
                &v.p3_receipt,
                &result,
            )
        }
        "baseline.created" => {
            let v: BaselineCreated =
                serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
            let BaselineCreatedAction::RecordBaseline { result } = &v.request_body.action;
            object_id(&v.result.commit)?;
            sha256_hex(&v.result.content_hash)?;
            if result != &v.result
                || v.result.publication_id != v.result.approval_eid
                || v.result.tag != v.result.target
                || !v.result.tags.contains(&v.result.tag)
                || v.result.tags.windows(2).any(|p| p[0] >= p[1])
                || v.p3_request.request_id
                    != crate::events::derived_eid(&v.result.commit, "baseline.created.request")?
                || v.request_body.actor != "process:towermd-baseline"
            {
                return Err("baseline created request/result mismatch".into());
            }
            check_request(
                project,
                &v.request_body,
                &v.p3_request,
                &v.p3_receipt,
                &v.result,
            )
        }
        _ => Err("unsupported recorded data kind".into()),
    }
}
fn check_request<A: Serialize, T: PartialEq>(
    project: &str,
    body: &recorded::RequestBody<A>,
    meta: &recorded::RequestMeta,
    receipt: &recorded::RequestReceipt<T>,
    result: &T,
) -> Result<(), String> {
    crate::hub::validate_token("host", &body.host).map_err(|e| e.to_string())?;
    if body.project_id != project
        || body.run.is_some()
        || meta.body_hash != canonical_hash(body)?
        || receipt.request_id != meta.request_id
        || receipt.body_hash != meta.body_hash
        || &receipt.result_ref != result
    {
        return Err("original typed request/receipt mismatch".into());
    }
    Ok(())
}
fn human_actor(actor: &str) -> Result<(), String> {
    let id = actor
        .strip_prefix("human:")
        .filter(|s| !s.is_empty() && !s.contains([':', '/']))
        .ok_or("review requires original human actor")?;
    crate::hub::validate_token("human", id).map_err(|e| e.to_string())
}
#[allow(dead_code, reason = "E1/P3 consume this narrow opening layout")]
pub(crate) fn validate_review_open(value: &recorded::ReviewOpenView) -> Result<(), String> {
    use recorded::{ReviewBodyView, ReviewGate};
    let ReviewBodyView::Review {
        binding,
        verdict,
        plan_content,
        comment,
        ..
    } = &value.hold.body;
    if value.hold.object_type != "Hold"
        || value.hold.reply.is_some()
        || verdict.is_some()
        || comment.is_some()
        || binding != &value.binding
        || plan_content != &value.plan_content
    {
        return Err("opening must preserve unanswered original Hold".into());
    }
    match (&value.gate, &binding.subject, &value.plan_content) {
        (ReviewGate::G1, task_review::ReviewSubject::Plan { sha256: hash }, Some(plan))
            if sha256(plan.as_bytes()) == *hash =>
        {
            Ok(())
        }
        (ReviewGate::G4, task_review::ReviewSubject::Tree { tree }, None) => object_id(tree),
        _ => Err("opening gate/subject/plan content mismatch".into()),
    }
}
#[allow(dead_code, reason = "E1/P2 consume the shared gate evidence layout")]
pub(crate) fn validate_gate_layout(value: &recorded::GateEvidence) -> Result<(), String> {
    if value.object_type != "Evidence"
        || value.kind != "gate"
        || value.generated.by != "process:towermd-gates"
    {
        return Err("invalid gate Evidence identity/producer".into());
    }
    let id = value
        .id
        .strip_prefix("E-")
        .ok_or("gate Evidence lacks UUIDv7 identity")?;
    recorded::request_event_eid(id, "evidence.recorded")?;
    chrono::DateTime::parse_from_rfc3339(&value.generated.at).map_err(|e| e.to_string())?;
    canonical_json_bytes(value)?;
    match value.gate {
        crate::task_gates::Gate::G1 | crate::task_gates::Gate::G4 => {
            for oid in [&value.commit, &value.tree, &value.base, &value.authority] {
                object_id(oid.as_deref().ok_or("review gate lacks Git context")?)?;
            }
            if value.baseline.as_ref().is_none_or(String::is_empty) {
                return Err("review gate lacks baseline".into());
            }
            sha256_hex(
                value
                    .commands_hash
                    .as_deref()
                    .ok_or("review gate lacks commands hash")?,
            )?;
            let review = value
                .review
                .as_ref()
                .ok_or("review gate lacks original review events")?;
            match value.gate {
                crate::task_gates::Gate::G1 => {
                    sha256_hex(value.plan_hash.as_deref().ok_or("G1 lacks plan hash")?)?;
                    if value.plan_opening_eid.is_none() || review.completed_eid.is_some() {
                        return Err("G1 opening/completion context mismatch".into());
                    }
                }
                crate::task_gates::Gate::G4 => {
                    if review.completed_eid.is_none() {
                        return Err("G4 lacks original completion reference".into());
                    }
                }
                _ => unreachable!(),
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn deserialize_event<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<crate::hub_protocol::Event, D::Error> {
    #[derive(Deserialize)]
    struct Actual {
        seq: u64,
        eid: String,
        host: String,
        t: String,
        actor: String,
        #[serde(rename = "type")]
        kind: String,
        layer: String,
        obj: String,
        data: serde_json::Value,
        #[serde(deserialize_with = "crate::recovery_contract::required_nullable")]
        epoch: Option<u64>,
        stale: bool,
        #[serde(deserialize_with = "crate::recovery_contract::required_nullable")]
        cost_usd: Option<f64>,
        #[serde(deserialize_with = "crate::recovery_contract::deserialize_fences")]
        guards: Vec<crate::hub_protocol::Fence>,
        #[serde(default)]
        lease_delta: Option<crate::hub::SystemDelta>,
        #[serde(default)]
        heartbeat: bool,
        #[serde(default)]
        legacy: bool,
        #[serde(default)]
        legacy_change: Option<crate::hub_legacy::LeaseChange>,
        #[serde(flatten)]
        extra: std::collections::BTreeMap<String, serde_json::Value>,
    }
    let e = Actual::deserialize(d)?;
    Ok(crate::hub_protocol::Event {
        seq: e.seq,
        eid: e.eid,
        host: e.host,
        t: e.t,
        actor: e.actor,
        kind: e.kind,
        layer: e.layer,
        obj: e.obj,
        data: e.data,
        epoch: e.epoch,
        stale: e.stale,
        cost_usd: e.cost_usd,
        guards: e.guards,
        lease_delta: e.lease_delta,
        heartbeat: e.heartbeat,
        legacy: e.legacy,
        legacy_change: e.legacy_change,
        extra: e.extra,
    })
}

pub(crate) fn deserialize_fences<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Vec<crate::hub_protocol::Fence>, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Fence {
        name: String,
        holder: String,
        epoch: u64,
    }
    Ok(Vec::<Fence>::deserialize(d)?
        .into_iter()
        .map(|f| crate::hub_protocol::Fence {
            name: f.name,
            holder: f.holder,
            epoch: f.epoch,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use finish::*;
    use serde_json::json;
    use verification::*;
    fn completion() -> (FrozenFinish, FinishEvidence, VerificationReport) {
        let command = VerificationCommand {
            cwd: ".".into(),
            argv: vec!["check".into()],
        };
        let input = FrozenFinish {
            evidence_id: "E".into(),
            project_id: "p".into(),
            host: "h".into(),
            root: "/old/root".into(),
            common_dir: "/old/common".into(),
            task_id: "T".into(),
            run_id: "R".into(),
            module_id: "M".into(),
            commit: "a".repeat(40),
            tree: "b".repeat(40),
            base: "c".repeat(40),
            authority: "c".repeat(40),
            baseline: "towermd/B0".into(),
            plan_hash: None,
            commands: vec![command.clone()],
            commands_hash: commands_hash(std::slice::from_ref(&command)).unwrap(),
            fences: vec![],
            entered_verifying_at: 1,
            merge_deadline: 2,
        };
        let report = VerificationReport {
            commit: input.commit.clone(),
            tree: input.tree.clone(),
            executions: vec![CommandExecution {
                command,
                result: "passed".into(),
                exit_code: Some(0),
                timed_out: false,
                stdout_log: "old/absent/stdout".into(),
                stderr_log: "old/absent/stderr".into(),
                stdout_sha256: sha256(b"out"),
                stderr_sha256: sha256(b"err"),
                diagnostic: None,
            }],
        };
        let evidence = FinishEvidence {
            id: input.evidence_id.clone(),
            kind: "test".into(),
            task: input.task_id.clone(),
            run: input.run_id.clone(),
            module: input.module_id.clone(),
            commit: input.commit.clone(),
            tree: input.tree.clone(),
            base: input.base.clone(),
            authority: input.authority.clone(),
            baseline: input.baseline.clone(),
            plan_hash: input.plan_hash.clone(),
            commands_hash: input.commands_hash.clone(),
            fences: input.fences.clone(),
            result: EvidenceResult::Passed,
            generated_by: "process:towermd-verifier".into(),
            log: VerificationLogRef {
                report_eid: "completed".into(),
                report_sha256: report_hash(&report).unwrap(),
            },
            gates: std::collections::BTreeMap::from([("G3".into(), true)]),
            extra: std::collections::BTreeMap::from([("future".into(), json!({"a":[2,1]}))]),
        };
        (input, evidence, report)
    }
    #[test]
    fn typed_completion_binds_report_log_eid_hash_and_entire_freeze() {
        let (input, evidence, report) = completion();
        assert!(validate_finish_report(&input, "completed", &evidence, &report).is_ok());
        let value = serde_json::to_value(&evidence).unwrap();
        for (key, bad) in [
            ("id", json!("different")),
            ("task", json!("different")),
            ("run", json!("different")),
            ("module", json!("different")),
            ("commit", json!("d".repeat(40))),
            ("tree", json!("d".repeat(40))),
            ("base", json!("d".repeat(40))),
            ("authority", json!("d".repeat(40))),
            ("baseline", json!("towermd/B1")),
            ("plan_hash", json!("d".repeat(64))),
            ("commands_hash", json!("d".repeat(64))),
            ("fences", json!([{"name":"main","holder":"x","epoch":7}])),
            ("generated_by", json!("process:other")),
            (
                "log",
                json!({"report_eid":"other","report_sha256":report_hash(&report).unwrap()}),
            ),
        ] {
            let mut bad_evidence = value.clone();
            bad_evidence[key] = bad;
            let bad_evidence = serde_json::from_value(bad_evidence).unwrap();
            assert!(
                validate_finish_report(&input, "completed", &bad_evidence, &report).is_err(),
                "{key}"
            );
        }
        let mut bad = evidence.clone();
        bad.log.report_sha256 = "0".repeat(64);
        assert!(validate_finish_report(&input, "completed", &bad, &report).is_err());
    }
    #[test]
    fn typed_completion_rejects_wrong_commit_tree_or_complete_command_sequence() {
        let (input, mut evidence, report) = completion();
        for field in ["commit", "tree", "command"] {
            let mut bad = report.clone();
            match field {
                "commit" => bad.commit = "d".repeat(40),
                "tree" => bad.tree = "d".repeat(40),
                _ => bad.executions[0].command.argv.reverse(),
            };
            if field == "command" {
                bad.executions[0].command.argv.push("unapproved".into());
            }
            evidence.log.report_sha256 = report_hash(&bad).unwrap();
            assert!(
                validate_finish_report(&input, "completed", &evidence, &bad).is_err(),
                "{field}"
            );
        }
    }
    #[test]
    fn passed_completion_requires_real_nonempty_successful_execution_records() {
        let (input, mut evidence, report) = completion();
        for field in ["result", "exit", "timeout", "empty", "loghash"] {
            let mut bad = report.clone();
            match field {
                "result" => bad.executions[0].result = "failed".into(),
                "exit" => bad.executions[0].exit_code = None,
                "timeout" => bad.executions[0].timed_out = true,
                "empty" => bad.executions.clear(),
                _ => bad.executions[0].stdout_sha256 = "invalid".into(),
            };
            evidence.log.report_sha256 = report_hash(&bad).unwrap();
            assert!(
                validate_finish_report(&input, "completed", &evidence, &bad).is_err(),
                "{field}"
            );
        }
    }
    fn typed_recorded_values() -> Vec<serde_json::Value> {
        let tb = json!({"run":"R","task":"T","baseline":"towermd/B0","fences":[],"subject":{"kind":"plan","sha256":"a".repeat(64)}});
        let fact =
            json!({"binding":tb,"verdict":"approved","actor":"human:u","answer_eid":"event"});
        let bb = json!({"project_id":"p","object_id":"d","content_hash":"a".repeat(64),"target_baseline":"towermd/B0","draft_revision":"b".repeat(40)});
        let approval = json!({"approval_eid":"approved","binding":bb,"actor":"human:u","verdict":"approved","comment":"ok"});
        let prepared = json!({"prepared_eid":"prepared","publication_id":"approved","authority_revision":"b".repeat(40),"binding":bb,"content_hash":"a".repeat(64)});
        let result = json!({"commit":"c".repeat(40),"publication_id":"approved","approval_eid":"approved","content_hash":"a".repeat(64),"target":"towermd/B0","tag":"towermd/B0","tags":["towermd/B0"]});
        let body = |a| json!({"project_id":"p","actor":"human:u","host":"h","run":null,"action":a});
        let request = json!({"request_id":"u","body_hash":"a".repeat(64)});
        let receipt = |r| json!({"request_id":"u","body_hash":"a".repeat(64),"result_ref":r});
        vec![
            json!({"hold":{"type":"Hold","id":"H","body":{"kind":"review","binding":tb,"verdict":"approved","plan_content":"plan","source_commit":null,"source_path":null,"comment":"ok"},"opened_ms":1,"opened_seq":1,"reply":{"id":"reply","text":"ok","actor":"human:u","at_ms":2,"future_reply":42},"future_hold":42},"fact":fact,"comment":"ok","request_body":body(json!({"kind":"review","hold":"H","binding":tb,"verdict":"approved","comment":"ok"})),"p3_request":request,"p3_receipt":receipt(json!({"hold":"H","fact":fact,"verdict":"approved","comment":"ok"}))}),
            json!({"approval":approval,"request_body":body(json!({"kind":"review_baseline","binding":bb,"verdict":"approved","comment":"ok"})),"p3_request":request,"p3_receipt":receipt(approval)}),
            json!({"publication_id":"approved","approval_eid":"approved","binding":bb,"authority_revision":"b".repeat(40),"content_hash":"a".repeat(64),"request_body":body(json!({"kind":"prepare_publication","publication_id":"approved","approval_eid":"approved","binding":bb,"authority_revision":"b".repeat(40),"content_hash":"a".repeat(64)})),"p3_request":request,"p3_receipt":receipt(prepared)}),
            {
                let mut v = result.clone();
                v["request_body"] = body(json!({"kind":"record_baseline","result":result}));
                v["p3_request"] = request.clone();
                v["p3_receipt"] = receipt(result);
                v
            },
        ]
    }

    fn valid_recorded_values() -> Vec<serde_json::Value> {
        let mut values = typed_recorded_values();
        let u = "0192f0aa-0000-7000-8000-000000000001";
        let approved = recorded::request_event_eid(u, "baseline.approved").unwrap();
        values[0]["fact"]["answer_eid"] =
            json!(recorded::request_event_eid(u, "hold.reviewed").unwrap());
        values[0]["p3_receipt"]["result_ref"]["fact"] = values[0]["fact"].clone();
        values[1]["approval"]["approval_eid"] = json!(approved);
        values[1]["p3_receipt"]["result_ref"] = values[1]["approval"].clone();
        for key in ["publication_id", "approval_eid"] {
            values[2][key] = json!(approved);
            values[2]["request_body"]["action"][key] = json!(approved);
            values[3][key] = json!(approved);
        }
        values[2]["p3_receipt"]["result_ref"]["publication_id"] = json!(approved);
        values[2]["p3_receipt"]["result_ref"]["prepared_eid"] =
            json!(recorded::request_event_eid(u, "baseline.publication_prepared").unwrap());
        let result=serde_json::to_value(serde_json::from_value::<recorded::BaselineCreatedResult>(json!({"commit":values[3]["commit"],"publication_id":approved,"approval_eid":approved,"content_hash":values[3]["content_hash"],"target":values[3]["target"],"tag":values[3]["tag"],"tags":values[3]["tags"]})).unwrap()).unwrap();
        values[3]["request_body"]["action"]["result"] = result.clone();
        values[3]["p3_receipt"]["result_ref"] = result;
        for (index, value) in values.iter_mut().enumerate() {
            if index >= 2 {
                value["request_body"]["actor"] = json!("process:towermd-baseline");
            }
            let id = if index == 3 {
                crate::events::derived_eid(
                    value["commit"].as_str().unwrap(),
                    "baseline.created.request",
                )
                .unwrap()
            } else {
                u.into()
            };
            let hash = canonical_hash(&value["request_body"]).unwrap();
            value["p3_request"] = json!({"request_id":id,"body_hash":hash});
            value["p3_receipt"]["request_id"] = json!(id);
            value["p3_receipt"]["body_hash"] = json!(hash);
        }
        values
    }
    fn recorded_case(index: usize, kind: &str) {
        let value = valid_recorded_values().remove(index);
        assert!(validate_recorded_data("p", kind, &value).is_ok());
        for pointer in [
            "/p3_request/body_hash",
            "/p3_receipt/body_hash",
            "/p3_receipt/request_id",
            "/request_body/host",
            "/request_body/actor",
            "/request_body/project_id",
        ] {
            let mut bad = value.clone();
            *bad.pointer_mut(pointer).unwrap() = json!("different");
            assert!(
                validate_recorded_data("p", kind, &bad).is_err(),
                "{kind} {pointer}"
            );
        }
        let result = value["p3_receipt"]["result_ref"].as_object().unwrap();
        for key in result.keys() {
            let mut bad = value.clone();
            bad["p3_receipt"]["result_ref"][key] = json!("different");
            assert!(
                validate_recorded_data("p", kind, &bad).is_err(),
                "{kind} receipt.{key}"
            );
        }
        let mut bad = value.clone();
        bad["request_body"]["run"] = json!("later-bound-run");
        let hash = canonical_hash(&bad["request_body"]).unwrap();
        bad["p3_request"]["body_hash"] = json!(hash);
        bad["p3_receipt"]["body_hash"] = json!(hash);
        assert!(validate_recorded_data("p", kind, &bad).is_err());
    }
    #[test]
    fn task_review_request_and_typed_original_receipt_match_all_fields() {
        recorded_case(0, "hold.reviewed");
    }
    #[test]
    fn baseline_review_request_and_typed_original_receipt_match_all_fields() {
        recorded_case(1, "baseline.approved");
    }
    #[test]
    fn publication_prepared_request_and_typed_original_receipt_match_all_fields() {
        recorded_case(2, "baseline.publication_prepared");
    }
    #[test]
    fn baseline_created_request_and_typed_original_receipt_match_all_fields() {
        recorded_case(3, "baseline.created");
    }
    #[test]
    fn review_open_layout_requires_unanswered_exact_subject_plan_and_gate() {
        let values = typed_recorded_values();
        let mut hold = values[0]["hold"].clone();
        hold["reply"] = json!(null);
        hold["body"]["verdict"] = json!(null);
        hold["body"]["comment"] = json!(null);
        let plan = "plan";
        hold["body"]["binding"]["subject"]["sha256"] = json!(sha256(plan.as_bytes()));
        let value =
            json!({"hold":hold,"gate":"g1","binding":hold["body"]["binding"],"plan_content":plan});
        let view = serde_json::from_value(value.clone()).unwrap();
        assert!(validate_review_open(&view).is_ok());
        for (pointer, bad) in [
            ("/gate", json!("g4")),
            ("/plan_content", json!("different")),
            ("/hold/body/comment", json!("answered")),
            ("/binding/task", json!("other")),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).unwrap() = bad;
            let view = serde_json::from_value(changed).unwrap();
            assert!(validate_review_open(&view).is_err(), "{pointer}");
        }
    }

    fn gate_value() -> serde_json::Value {
        // Narrow contract layout fixture, not a producer or an accepted review proof.
        let event = json!({"seq":1,"eid":"0192f0aa-0000-7000-8000-000000000001","host":"h","t":"1970-01-01T00:00:00.001Z","actor":"process:fixture","type":"fixture","layer":"L5","obj":"H","data":{},"epoch":null,"stale":false,"cost_usd":null,"guards":[]});
        json!({"type":"Evidence","id":"E-0192f0aa-0000-7000-8000-000000000001","kind":"gate","gate":"g1","project_id":"p","task":"T","run":"R","module":"M","commit":"a".repeat(40),"tree":"b".repeat(40),"base":"c".repeat(40),"authority":"c".repeat(40),"baseline":"towermd/B0","plan_hash":"a".repeat(64),"plan_opening_eid":"opening","commands_hash":"a".repeat(64),"fences":[],"result":"passed","review":{"opening":event,"reviewed":event,"completed_eid":null},"diagnostic":null,"generated":{"by":"process:towermd-gates","at":"1970-01-01T00:00:00.001Z"},"future":{"values":[2,1]}})
    }
    #[test]
    fn gate_layout_preserves_extra_and_requires_fixed_identity_and_review_context() {
        let value = gate_value();
        let gate: recorded::GateEvidence = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&gate).unwrap(), value);
        assert!(validate_gate_layout(&gate).is_ok());
        for (pointer, bad) in [
            ("/type", json!("Hold")),
            ("/kind", json!("test")),
            ("/gate", json!("g4")),
            ("/commands_hash", json!(null)),
            ("/review", json!(null)),
            ("/generated/by", json!("process:other")),
            ("/generated/at", json!("bad")),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).unwrap() = bad;
            let gate = serde_json::from_value(changed).unwrap();
            assert!(validate_gate_layout(&gate).is_err(), "{pointer}");
        }
    }
    #[test]
    fn gate_and_review_context_require_explicit_nullables_and_strict_fences() {
        let value = gate_value();
        for key in [
            "commit",
            "tree",
            "base",
            "authority",
            "baseline",
            "plan_hash",
            "plan_opening_eid",
            "commands_hash",
            "review",
            "diagnostic",
        ] {
            let mut bad = value.clone();
            bad.as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<recorded::GateEvidence>(bad).is_err(),
                "{key}"
            );
        }
        let mut bad = value.clone();
        bad["fences"] = json!([{"name":"main","holder":"writer","epoch":1,"future":true}]);
        assert!(serde_json::from_value::<recorded::GateEvidence>(bad).is_err());
        let mut bad = value.clone();
        bad["review"]["opening"]
            .as_object_mut()
            .unwrap()
            .remove("epoch");
        assert!(serde_json::from_value::<recorded::GateEvidence>(bad).is_err());
    }

    #[test]
    fn registered_evidence_extensions_cannot_override_fixed_fields() {
        let (_, mut evidence, _) = completion();
        evidence.extra.insert("tree".into(), json!("fake"));
        assert!(serde_json::to_value(evidence).is_err());
    }
}
