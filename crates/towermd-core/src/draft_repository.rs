//! Git materialization only. No result here certifies a business event or request receipt.
use super::{acquire_main, renew_fence, validate_authority_shared, SnapshotWriter};
use crate::{
    baseline_draft::{freeze, FrozenDraft},
    catalog::Catalog,
    project_session::ProjectSession,
    recovery_contract,
    recovery_contract::recorded::{RequestBody, RequestMeta},
    repository_writer::{self, RepositoryRef, Session, TreeFile},
    request_ledger::{DraftSaveAction, PreparedDraftSaveRequest},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt,
    sync::{Mutex, TryLockError},
};

const MANIFEST: &str = ".towermd/draft-manifest.json";

pub use crate::request_ledger::{DraftDelivery, DraftSavedResult};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DraftMaterialization {
    result: DraftSavedResult,
}
impl DraftMaterialization {
    pub fn result(&self) -> &DraftSavedResult {
        &self.result
    }
    pub fn revision(&self) -> &str {
        &self.result.revision
    }
    pub fn reference(&self) -> &str {
        &self.result.reference
    }
    pub fn content_hash(&self) -> &str {
        &self.result.content_hash
    }
    pub fn delivery(&self) -> &DraftDelivery {
        &self.result.delivery
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraftSaveError {
    Busy,
    Conflict { reason: String },
    Uncertain { reason: String },
    Refused { reason: String },
    BeforeMaterialization { reason: String },
}
impl fmt::Display for DraftSaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => f.write_str("repository writer is busy"),
            Self::Conflict { reason }
            | Self::Uncertain { reason }
            | Self::Refused { reason }
            | Self::BeforeMaterialization { reason } => f.write_str(reason),
        }
    }
}
impl std::error::Error for DraftSaveError {}
impl From<String> for DraftSaveError {
    fn from(reason: String) -> Self {
        Self::BeforeMaterialization { reason }
    }
}
fn conflict(reason: impl Into<String>) -> DraftSaveError {
    DraftSaveError::Conflict {
        reason: reason.into(),
    }
}
fn refused(reason: impl Into<String>) -> DraftSaveError {
    DraftSaveError::Refused {
        reason: reason.into(),
    }
}

#[derive(Clone, Debug)]
pub struct SavedDraft {
    revision: String,
    request: PreparedDraftSaveRequest,
    frozen: FrozenDraft,
}
impl SavedDraft {
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn request(&self) -> &PreparedDraftSaveRequest {
        &self.request
    }
    pub fn request_id(&self) -> &str {
        &self.request.metadata().request_id
    }
    pub fn expected_revision(&self) -> Option<&str> {
        self.request.expected_revision()
    }
    pub fn frozen(&self) -> &FrozenDraft {
        &self.frozen
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    request_body: RequestBody<DraftSaveAction>,
    p3_request: RequestMeta,
    frozen: FrozenDraft,
}

impl SnapshotWriter {
    /// Materialize a private draft under the same writer/main lease as snapshots.
    /// The caller must separately persist its original business event and receipt.
    pub fn save_draft(
        &self,
        session: &Mutex<ProjectSession>,
        catalog: &Catalog,
        draft: &FrozenDraft,
        request: &PreparedDraftSaveRequest,
        mut now: impl FnMut() -> Result<u64, String>,
    ) -> Result<DraftMaterialization, DraftSaveError> {
        let _writing = match self.writing.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Err(DraftSaveError::Busy),
            Err(TryLockError::Poisoned(_)) => {
                return Err(refused("repository writer lock poisoned"))
            }
        };
        let request_id = &request.metadata().request_id;
        let expected_revision = request.expected_revision();
        let host = request.body().host.as_str();
        if request.draft_id() != draft.bundle.id
            || request.body().project_id != draft.bundle.project_id
        {
            return Err(refused("typed save request differs from draft id/project"));
        }
        if let DraftSaveAction::CreateDraft {
            baseline,
            main_revision,
        } = &request.body().action
        {
            validate_create_source(catalog, draft, baseline, main_revision)?;
        }
        if let DraftSaveAction::SubmitBaseline { submit } = &request.body().action {
            if submit.source_revision != draft.bundle.source_revision {
                return Err(refused("typed submit source differs from frozen draft"));
            }
        }
        let target = RepositoryRef::draft(&draft.bundle.id).map_err(refused)?;
        if let Some(expected) = expected_revision {
            recovery_contract::object_id(expected).map_err(refused)?;
        }
        current_catalog(catalog)?;
        let reader = Session::for_network(&catalog.root)?;
        let existing =
            repository_writer::direct_revision(&catalog.root, &target).map_err(refused)?;
        let replay = match &existing {
            Some(revision) => {
                let saved = read_at(catalog, &draft.bundle.id, revision, &reader)?;
                if saved.request_id() == request_id {
                    if saved.request() != request || saved.frozen != *draft {
                        return Err(conflict("request_id was materialized with different draft content or expected revision"));
                    }
                    true
                } else {
                    if expected_revision != Some(revision.as_str()) {
                        return Err(conflict("draft expected revision changed"));
                    }
                    if saved.frozen.bundle.source_revision != draft.bundle.source_revision {
                        return Err(conflict("draft source cannot change across revisions"));
                    }
                    false
                }
            }
            None => {
                if expected_revision.is_some() {
                    return Err(conflict("draft expected revision is absent"));
                }
                false
            }
        };
        validate_frozen(catalog, draft)?;
        validate_authority_shared(session, catalog).map_err(refused)?;
        let fence = {
            let mut live = session
                .lock()
                .map_err(|_| refused("session lock poisoned"))?;
            live.bind_catalog(catalog).map_err(refused)?;
            acquire_main(&mut live, host, now()?).map_err(refused)?
        };
        let git = Session::for_draft(&catalog.root)?;
        let mut verify = || -> Result<(), DraftSaveError> {
            current_catalog(catalog)?;
            validate_frozen(catalog, draft)?;
            validate_authority_shared(session, catalog).map_err(refused)?;
            let mut live = session
                .lock()
                .map_err(|_| refused("session lock poisoned"))?;
            live.bind_catalog(catalog).map_err(refused)?;
            renew_fence(&mut live, host, &fence, now()?).map_err(refused)
        };
        let revision = if replay {
            verify()?;
            existing.as_ref().unwrap().clone()
        } else {
            let manifest = Manifest {
                schema: 1,
                request_body: request.body().clone(),
                p3_request: request.metadata().clone(),
                frozen: draft.clone(),
            };
            let manifest = manifest_bytes(&manifest)?;
            let mut files: Vec<_> = draft
                .files
                .iter()
                .map(|(path, file)| TreeFile {
                    path,
                    mode: &file.mode,
                    content: &file.text,
                })
                .collect();
            files.push(TreeFile {
                path: MANIFEST,
                mode: "100644",
                content: &manifest,
            });
            if !git
                .run(
                    &catalog.root,
                    &[
                        "ls-tree",
                        "-z",
                        &draft.bundle.source_revision,
                        "--",
                        MANIFEST,
                    ],
                    None,
                )?
                .is_empty()
            {
                return Err(refused("draft manifest path already exists in source tree"));
            }
            let tree = git.build_tree(
                &catalog.root,
                &draft.bundle.source_revision,
                &files,
                &draft.bundle.deletions,
            )?;
            verify()?;
            unchanged_ref(catalog, &target, existing.as_deref())?;
            let mut parents = vec![draft.bundle.source_revision.as_str()];
            if let Some(previous) =
                expected_revision.filter(|previous| *previous != draft.bundle.source_revision)
            {
                parents.push(previous);
            }
            let message = format!("Tower.md draft\n\nTowermd-Draft: {}\nTowermd-Request: {request_id}\nTowermd-Content: {}", draft.bundle.id, draft.content_hash);
            let revision = git.commit_tree(&catalog.root, &tree, &parents, &message)?;
            #[cfg(test)]
            crate::snapshot_git::tests::observe(&catalog.root, &["before-draft-cas-validation"]);
            verify()?;
            unchanged_ref(catalog, &target, existing.as_deref())?;
            let old = existing
                .clone()
                .unwrap_or_else(|| "0".repeat(revision.len()));
            git.cas_ref_checked(&catalog.root, &target, &revision, &old)
                .map_err(|failure| match failure {
                    repository_writer::CasRefFailure::Conflict { reason } => conflict(reason),
                    repository_writer::CasRefFailure::Uncertain { reason } => {
                        DraftSaveError::Uncertain {
                            reason: crate::request_ledger::uncertain_request(request_id, reason),
                        }
                    }
                })?;
            revision
        };
        // After CAS, failures retain and report the actual local materialization.
        let remote = repository_writer::read_git(&catalog.root, &["remote"]);
        let delivery = match remote {
            Ok(remotes) if !remotes.lines().any(|name| name == "origin") => {
                DraftDelivery::LocalSaved
            }
            remote => {
                let pushed = (|| -> Result<(), String> {
                    remote?;
                    verify().map_err(|e| e.to_string())?;
                    unchanged_ref(catalog, &target, Some(&revision)).map_err(|e| e.to_string())?;
                    let url = git.origin_push_url(&catalog.root)?;
                    let output = git.push_commit(&catalog.root, &url, &revision, &target)?;
                    if !output.status.success() {
                        return Err(format!(
                            "draft push not confirmed: {} {}",
                            String::from_utf8_lossy(&output.stdout),
                            String::from_utf8_lossy(&output.stderr)
                        ));
                    }
                    Ok(())
                })();
                match pushed {
                    Ok(()) => DraftDelivery::RemoteSaved,
                    Err(reason) => DraftDelivery::LocalSavedRemotePending { reason },
                }
            }
        };
        Ok(DraftMaterialization {
            result: DraftSavedResult {
                draft_id: draft.bundle.id.clone(),
                revision,
                reference: target.name().into(),
                content_hash: draft.content_hash.clone(),
                delivery,
            },
        })
    }
}

/// Read a fixed private ref and verify its actual tree/parents and current project
/// context. Reading a manifest never constructs a business fact or approval.
pub fn read_draft(catalog: &Catalog, id: &str) -> Result<Option<SavedDraft>, DraftSaveError> {
    let target = RepositoryRef::draft(id).map_err(refused)?;
    current_catalog(catalog)?;
    let Some(revision) =
        repository_writer::direct_revision(&catalog.root, &target).map_err(refused)?
    else {
        return Ok(None);
    };
    let git = Session::for_network(&catalog.root)?;
    let saved = read_at(catalog, id, &revision, &git)?;
    current_catalog(catalog)?;
    unchanged_ref(catalog, &target, Some(&revision))?;
    Ok(Some(saved))
}

fn user_draft_refs(catalog: &Catalog) -> Result<Vec<(String, String)>, DraftSaveError> {
    let output = repository_writer::read_git(
        &catalog.root,
        &[
            "for-each-ref",
            "--format=%(refname)%09%(objectname)%09%(symref)",
            "refs/towermd/drafts/",
        ],
    )?;
    let mut refs = Vec::new();
    for line in output.lines() {
        let mut fields = line.split('\t');
        let reference = fields
            .next()
            .ok_or_else(|| refused("draft ref lacks name"))?;
        let revision = fields
            .next()
            .ok_or_else(|| refused("draft ref lacks revision"))?;
        let symbolic = fields.next().unwrap_or_default();
        let id = reference
            .strip_prefix("refs/towermd/drafts/")
            .ok_or_else(|| refused("draft ref outside namespace"))?;
        if id.starts_with("publication-") {
            continue;
        }
        if !symbolic.is_empty() || fields.next().is_some() {
            return Err(refused("user draft ref must be direct"));
        }
        RepositoryRef::draft(id).map_err(refused)?;
        recovery_contract::object_id(revision).map_err(refused)?;
        refs.push((id.into(), revision.into()));
    }
    refs.sort();
    Ok(refs)
}

/// Observation metadata only, used to invalidate a derived request index when a
/// private draft changes without advancing main.
pub(crate) fn draft_ref_fingerprint(catalog: &Catalog) -> Result<String, DraftSaveError> {
    recovery_contract::canonical_hash(&user_draft_refs(catalog)?).map_err(refused)
}

/// Actual current user drafts, including saves whose business event was not yet
/// accepted. Publication candidates are a separate, reserved namespace.
pub(crate) fn read_all_drafts(catalog: &Catalog) -> Result<Vec<SavedDraft>, DraftSaveError> {
    current_catalog(catalog)?;
    let refs = user_draft_refs(catalog)?;
    let git = Session::for_network(&catalog.root)?;
    let mut saved = Vec::new();
    for (id, revision) in &refs {
        saved.push(read_at(catalog, id, revision, &git)?);
    }
    current_catalog(catalog)?;
    if user_draft_refs(catalog)? != refs {
        return Err(conflict("draft ref set changed during request preparation"));
    }
    Ok(saved)
}
fn current_catalog(catalog: &Catalog) -> Result<(), DraftSaveError> {
    if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
        return Err(conflict("authority changed; reload Catalog"));
    }
    crate::recovery_outbox::RecoveryContext::from_catalog(catalog).map_err(refused)?;
    Ok(())
}
fn validate_frozen(catalog: &Catalog, frozen: &FrozenDraft) -> Result<(), DraftSaveError> {
    let computed = freeze(catalog, &frozen.bundle).map_err(refused)?;
    if computed != *frozen {
        return Err(refused(
            "FrozenDraft differs from actual source-derived paths/modes/bytes/hash",
        ));
    }
    Ok(())
}
fn validate_create_source(
    catalog: &Catalog,
    frozen: &FrozenDraft,
    baseline: &str,
    main_revision: &str,
) -> Result<(), DraftSaveError> {
    if frozen.bundle.source_revision != main_revision {
        return Err(refused("typed create source differs from frozen draft"));
    }
    let source = Catalog::at_revision(&catalog.root, main_revision)?
        .ok_or_else(|| refused("create source lacks project configuration"))?;
    let imported =
        crate::baseline_draft::current_source_bundle(&source, &frozen.bundle.id, baseline)
            .map_err(refused)?;
    if imported != frozen.bundle {
        return Err(refused(
            "typed create differs from exact source objects/canvas/target",
        ));
    }
    Ok(())
}

fn validate_frozen_source(catalog: &Catalog, frozen: &FrozenDraft) -> Result<(), DraftSaveError> {
    let source = Catalog::at_revision(&catalog.root, &frozen.bundle.source_revision)?
        .ok_or_else(|| refused("draft source lacks project configuration"))?;
    if source.common_dir != catalog.common_dir
        || source.config.to_yaml() != catalog.config.to_yaml()
    {
        return Err(refused(
            "draft source project/configuration/permissions changed",
        ));
    }
    let computed =
        crate::baseline_draft::freeze_source(&source, &frozen.bundle).map_err(refused)?;
    if computed != *frozen {
        return Err(refused(
            "FrozenDraft differs from actual source-derived paths/modes/bytes/hash",
        ));
    }
    Ok(())
}
fn unchanged_ref(
    catalog: &Catalog,
    target: &RepositoryRef,
    expected: Option<&str>,
) -> Result<(), DraftSaveError> {
    if repository_writer::direct_revision(&catalog.root, target)
        .map_err(refused)?
        .as_deref()
        != expected
    {
        return Err(conflict("draft ref changed during materialization/read"));
    }
    Ok(())
}
fn manifest_bytes(manifest: &Manifest) -> Result<String, DraftSaveError> {
    serde_json::to_string_pretty(manifest)
        .map(|text| format!("{text}\n"))
        .map_err(|e| e.to_string().into())
}
fn entry(
    git: &Session,
    catalog: &Catalog,
    revision: &str,
    path: &str,
) -> Result<Option<(String, String)>, DraftSaveError> {
    let entries = git.run(
        &catalog.root,
        &["ls-tree", "-z", revision, "--", path],
        None,
    )?;
    if entries.is_empty() {
        return Ok(None);
    }
    let mut entries = entries.split('\0').filter(|entry| !entry.is_empty());
    let entry = entries
        .next()
        .ok_or_else(|| refused("missing exact tree entry"))?;
    if entries.next().is_some() {
        return Err(refused("ambiguous draft tree entry"));
    }
    let (metadata, actual) = entry
        .split_once('\t')
        .ok_or_else(|| refused("invalid draft tree entry"))?;
    let fields: Vec<_> = metadata.split_whitespace().collect();
    if fields.len() != 3
        || actual != path
        || fields[1] != "blob"
        || !matches!(fields[0], "100644" | "100755")
    {
        return Err(refused("draft path is not an exact regular blob"));
    }
    recovery_contract::object_id(fields[2]).map_err(refused)?;
    Ok(Some((fields[0].into(), fields[2].into())))
}
fn read_at(
    catalog: &Catalog,
    id: &str,
    revision: &str,
    git: &Session,
) -> Result<SavedDraft, DraftSaveError> {
    let (mode, blob) = entry(git, catalog, revision, MANIFEST)?
        .ok_or_else(|| refused("draft ref lacks manifest"))?;
    if mode != "100644" {
        return Err(refused("draft manifest mode must be 100644"));
    }
    let bytes = crate::repo::git_bytes_timeout(
        &catalog.root,
        &["cat-file", "blob", &blob],
        repository_writer::TIMEOUT,
    )?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| refused(format!("invalid draft manifest: {e}")))?;
    if manifest.schema != 1 || manifest.frozen.bundle.id != id {
        return Err(refused("draft manifest schema/id mismatch"));
    }
    let request = PreparedDraftSaveRequest::from_recorded(
        manifest.request_body.clone(),
        manifest.p3_request.clone(),
    )
    .map_err(refused)?;
    if request.draft_id() != id || request.body().project_id != manifest.frozen.bundle.project_id {
        return Err(refused(
            "manifest request differs from actual draft id/project",
        ));
    }
    if let DraftSaveAction::CreateDraft {
        baseline,
        main_revision,
    } = &request.body().action
    {
        validate_create_source(catalog, &manifest.frozen, baseline, main_revision)?;
    }
    if let DraftSaveAction::SubmitBaseline { submit } = &request.body().action {
        if submit.source_revision != manifest.frozen.bundle.source_revision {
            return Err(refused("manifest submit source differs from frozen draft"));
        }
    }
    validate_frozen_source(catalog, &manifest.frozen)?;
    if bytes != manifest_bytes(&manifest)?.as_bytes() {
        return Err(refused(
            "draft manifest is not the exact canonical materialization",
        ));
    }
    let source = &manifest.frozen.bundle.source_revision;
    if entry(git, catalog, source, MANIFEST)?.is_some() {
        return Err(refused("draft manifest was already present in source"));
    }
    let parents = git.run(
        &catalog.root,
        &["rev-list", "--parents", "-n", "1", revision],
        None,
    )?;
    let mut expected_parents = vec![revision, source.as_str()];
    if let Some(previous) = request
        .expected_revision()
        .filter(|previous| *previous != source)
    {
        expected_parents.push(previous);
    }
    if parents.split_whitespace().collect::<Vec<_>>() != expected_parents {
        return Err(refused(
            "draft commit parents differ from source/expected revision",
        ));
    }
    let allowed: BTreeSet<_> = manifest
        .frozen
        .files
        .keys()
        .map(String::as_str)
        .chain(manifest.frozen.bundle.deletions.iter().map(String::as_str))
        .chain([MANIFEST])
        .collect();
    git.validate_changes(&catalog.root, source, revision, &allowed)
        .map_err(refused)?;
    for (path, expected) in &manifest.frozen.files {
        let (mode, blob) = entry(git, catalog, revision, path)?
            .ok_or_else(|| refused("draft frozen file missing from actual tree"))?;
        let actual = crate::repo::git_bytes_timeout(
            &catalog.root,
            &["cat-file", "blob", &blob],
            repository_writer::TIMEOUT,
        )?;
        if mode != expected.mode || actual != expected.text.as_bytes() {
            return Err(refused(
                "draft actual file modes/bytes differ from manifest",
            ));
        }
    }
    for path in &manifest.frozen.bundle.deletions {
        if entry(git, catalog, revision, path)?.is_some() {
            return Err(refused("draft deletion remains in actual tree"));
        }
    }
    Ok(SavedDraft {
        revision: revision.into(),
        request,
        frozen: manifest.frozen,
    })
}

#[cfg(test)]
#[path = "draft_repository_tests.rs"]
mod tests;
