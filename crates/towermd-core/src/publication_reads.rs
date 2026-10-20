//! One finite owner of immutable Git reads and completed Proposal context checks.
//! No verified publication, accepted role, ref lookup or ancestry result is cached.
use super::*;
use std::{path::PathBuf, sync::Arc};

pub(crate) struct PublicationReads {
    root: PathBuf,
    common: PathBuf,
    project: String,
    config: String,
    authority: String,
    private: Option<String>,
    refs: String,
    trees: BTreeMap<String, GitTreeEntries>,
    blobs: BTreeMap<String, String>,
    kinds: BTreeMap<String, String>,
    catalogs: BTreeMap<String, Arc<Catalog>>,
    snapshots: BTreeMap<String, snapshot_service::SavedSnapshot>,
    saved_drafts: BTreeMap<String, snapshot_service::SavedDraft>,
    frozen_sources: BTreeMap<String, crate::baseline_draft::FrozenDraft>,
    messages: BTreeMap<String, String>,
    parents: BTreeMap<String, String>,
    completed_proposal_contexts: BTreeSet<String>,
    active_proposal_contexts: BTreeSet<String>,
    #[cfg(test)]
    counts: BTreeMap<(String, &'static str), usize>,
}
impl PublicationReads {
    pub(crate) fn new(catalog: &Catalog) -> Result<Self, String> {
        crate::recovery_outbox::RecoveryContext::from_catalog(catalog)?;
        let reads = Self {
            root: catalog.root.clone(),
            common: catalog.common_dir.clone(),
            project: catalog.config.project_id().into(),
            config: catalog.config.to_yaml(),
            authority: catalog.revision.clone(),
            private: crate::snapshot_git::private_revision(&catalog.root)?,
            refs: snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?,
            trees: BTreeMap::new(),
            blobs: BTreeMap::new(),
            kinds: BTreeMap::new(),
            catalogs: BTreeMap::new(),
            snapshots: BTreeMap::new(),
            saved_drafts: BTreeMap::new(),
            frozen_sources: BTreeMap::new(),
            messages: BTreeMap::new(),
            parents: BTreeMap::new(),
            completed_proposal_contexts: BTreeSet::new(),
            active_proposal_contexts: BTreeSet::new(),
            #[cfg(test)]
            counts: BTreeMap::new(),
        };
        reads.probe(catalog)?;
        Ok(reads)
    }
    fn bind(&self, catalog: &Catalog) -> Result<(), String> {
        if catalog.root != self.root
            || catalog.common_dir != self.common
            || catalog.config.project_id() != self.project
            || catalog.config.to_yaml() != self.config
            || catalog.revision != self.authority
        {
            return Err("finite publication reads project/source binding changed".into());
        }
        Ok(())
    }
    pub(crate) fn probe(&self, catalog: &Catalog) -> Result<(), String> {
        self.bind(catalog)?;
        crate::recovery_outbox::RecoveryContext::from_catalog(catalog)?;
        if crate::catalog::authority_revision(&catalog.root)? != self.authority
            || crate::snapshot_git::private_revision(&catalog.root)? != self.private
            || snapshot_service::draft_ref_fingerprint(catalog).map_err(|e| e.to_string())?
                != self.refs
        {
            return Err("finite publication reads authority/private/approval refs changed".into());
        }
        Ok(())
    }
    // Historical source revisions do not replace this owner's actual authority.
    // Only the immutable static readers below accept a separate source Catalog.
    fn static_context(&self, owner: &Catalog, source: &Catalog) -> Result<(), String> {
        self.probe(owner)?;
        if source.root != self.root
            || source.common_dir != self.common
            || source.config.project_id() != self.project
            || source.config.to_yaml() != self.config
        {
            return Err("finite static reads historical project/source context changed".into());
        }
        Ok(())
    }
    pub(crate) fn saved_draft(
        &mut self,
        owner: &Catalog,
        source: &Catalog,
        id: &str,
        revision: &str,
    ) -> Result<snapshot_service::SavedDraft, String> {
        self.static_context(owner, source)?;
        let key = contract::canonical_hash(&json!({
            "source_revision":source.revision,"source_config":source.config.to_yaml(),
            "id":id,"revision":revision
        }))?;
        let value = if let Some(saved) = self.saved_drafts.get(&key) {
            saved.clone()
        } else {
            self.missed(&key, "saved_draft");
            let saved = snapshot_service::read_draft_at(source, id, revision)
                .map_err(|error| error.to_string())?;
            self.probe(owner)?;
            self.saved_drafts.insert(key, saved.clone());
            saved
        };
        self.probe(owner)?;
        Ok(value)
    }
    pub(crate) fn frozen_source(
        &mut self,
        owner: &Catalog,
        source: &Catalog,
        bundle: &crate::baseline_draft::DraftBundle,
    ) -> Result<crate::baseline_draft::FrozenDraft, String> {
        self.static_context(owner, source)?;
        let key = contract::canonical_hash(&json!({
            "source_revision":source.revision,"source_config":source.config.to_yaml(),
            "bundle":bundle
        }))?;
        let value = if let Some(frozen) = self.frozen_sources.get(&key) {
            frozen.clone()
        } else {
            self.missed(&key, "frozen_source");
            let frozen = crate::baseline_draft::freeze_source(source, bundle)?;
            self.probe(owner)?;
            self.frozen_sources.insert(key, frozen.clone());
            frozen
        };
        self.probe(owner)?;
        Ok(value)
    }
    // These are completed predicate nodes, not history/role/capability values.
    // Only the full ProposalContext checker uses them in this finite owner.
    pub(super) fn begin_proposal_context(
        &mut self,
        catalog: &Catalog,
        key: &str,
    ) -> Result<bool, String> {
        self.probe(catalog)?;
        if self.completed_proposal_contexts.contains(key) {
            return Ok(true);
        }
        if !self.active_proposal_contexts.insert(key.into()) {
            return Err("cyclic finite Proposal context proof".into());
        }
        self.missed(key, "proposal_context");
        Ok(false)
    }
    pub(super) fn finish_proposal_context(
        &mut self,
        catalog: &Catalog,
        key: &str,
        result: Result<(), String>,
    ) -> Result<(), String> {
        if !self.active_proposal_contexts.remove(key) {
            return Err("finite Proposal context lacks its active proof".into());
        }
        result?;
        self.probe(catalog)?;
        self.completed_proposal_contexts.insert(key.into());
        Ok(())
    }
    fn missed(&mut self, oid: &str, kind: &'static str) {
        #[cfg(test)]
        {
            *self.counts.entry((oid.into(), kind)).or_default() += 1;
        }
        #[cfg(not(test))]
        {
            let _ = (oid, kind);
        }
    }
    pub(super) fn tree(&mut self, catalog: &Catalog, oid: &str) -> Result<GitTreeEntries, String> {
        self.bind(catalog)?;
        contract::object_id(oid)?;
        if let Some(value) = self.trees.get(oid) {
            return Ok(value.clone());
        }
        let value = super::tree(catalog, oid)?;
        self.missed(oid, "tree");
        self.trees.insert(oid.into(), value.clone());
        Ok(value)
    }
    fn kind(&mut self, catalog: &Catalog, oid: &str) -> Result<String, String> {
        self.bind(catalog)?;
        contract::object_id(oid)?;
        if let Some(value) = self.kinds.get(oid) {
            return Ok(value.clone());
        }
        let value = git(catalog, &["cat-file", "-t", oid])?;
        self.missed(oid, "kind");
        self.kinds.insert(oid.into(), value.clone());
        Ok(value)
    }
    pub(super) fn blob(&mut self, catalog: &Catalog, oid: &str) -> Result<String, String> {
        self.bind(catalog)?;
        contract::object_id(oid)?;
        if let Some(value) = self.blobs.get(oid) {
            return Ok(value.clone());
        }
        if self.kind(catalog, oid)? != "blob" {
            return Err("publication path is not a blob".into());
        }
        let value = String::from_utf8(git_bytes(catalog, &["cat-file", "blob", oid])?)
            .map_err(|e| e.to_string())?;
        self.missed(oid, "blob");
        self.blobs.insert(oid.into(), value.clone());
        Ok(value)
    }
    pub(super) fn file_text(
        &mut self,
        catalog: &Catalog,
        tree: &GitTreeEntries,
        path: &str,
    ) -> Result<String, String> {
        let (mode, oid) = tree
            .get(path)
            .ok_or_else(|| format!("missing publication file: {path}"))?;
        contract::file_mode(mode)?;
        self.blob(catalog, oid)
    }
    pub(super) fn catalog(&mut self, catalog: &Catalog, oid: &str) -> Result<Arc<Catalog>, String> {
        self.bind(catalog)?;
        contract::object_id(oid)?;
        if let Some(value) = self.catalogs.get(oid) {
            return Ok(value.clone());
        }
        let value = Arc::new(
            Catalog::at_revision(&catalog.root, oid)?.ok_or("publication source has no Catalog")?,
        );
        #[cfg(test)]
        super::tests::publication_local_reads::record(catalog, oid, "catalog");
        self.missed(oid, "catalog");
        self.catalogs.insert(oid.into(), value.clone());
        Ok(value)
    }
    pub(super) fn snapshot(
        &mut self,
        catalog: &Catalog,
        oid: &str,
    ) -> Result<snapshot_service::SavedSnapshot, String> {
        self.bind(catalog)?;
        contract::object_id(oid)?;
        if let Some(value) = self.snapshots.get(oid) {
            return Ok(value.clone());
        }
        let value = snapshot_service::read_snapshot_commit(catalog, oid)?;
        self.missed(oid, "snapshot");
        self.snapshots.insert(oid.into(), value.clone());
        Ok(value)
    }
    pub(super) fn message(&mut self, catalog: &Catalog, oid: &str) -> Result<String, String> {
        self.bind(catalog)?;
        contract::object_id(oid)?;
        if let Some(value) = self.messages.get(oid) {
            return Ok(value.clone());
        }
        let value = git(catalog, &["show", "--no-patch", "--format=%B", oid])?;
        self.missed(oid, "message");
        self.messages.insert(oid.into(), value.clone());
        Ok(value)
    }
    pub(super) fn parents(&mut self, catalog: &Catalog, oid: &str) -> Result<String, String> {
        self.bind(catalog)?;
        contract::object_id(oid)?;
        if let Some(value) = self.parents.get(oid) {
            return Ok(value.clone());
        }
        let value = git(catalog, &["rev-list", "--parents", "-n", "1", oid])?;
        self.missed(oid, "parents");
        self.parents.insert(oid.into(), value.clone());
        Ok(value)
    }
    pub(super) fn base_directory(
        &mut self,
        catalog: &Catalog,
        oid: &str,
    ) -> Result<BTreeMap<String, String>, String> {
        let at = self.catalog(catalog, oid)?;
        publication_base_context(catalog, &at)?;
        let entries = self.tree(catalog, oid)?;
        self.base_directory_in_tree(catalog, &at, &entries)
    }
    pub(super) fn base_directory_in_tree(
        &mut self,
        catalog: &Catalog,
        at: &Catalog,
        entries: &GitTreeEntries,
    ) -> Result<BTreeMap<String, String>, String> {
        self.bind(catalog)?;
        let prefix = format!("{}/", catalog.config.docs_dir());
        let mut result = BTreeMap::new();
        for (path, (mode, oid)) in entries {
            let directory = path
                .strip_prefix(&prefix)
                .is_some_and(|p| !p.starts_with("state/") && !p.starts_with("artifacts/"));
            if directory || path == crate::config::CONFIG_PATH || at.protected_paths.contains(path)
            {
                contract::file_mode(mode)?;
                if self.kind(catalog, oid)? != "blob" {
                    return Err("protected publication path is not a blob".into());
                }
                result.insert(path.clone(), format!("{mode} {oid}"));
            }
        }
        Ok(result)
    }
    #[cfg(test)]
    pub(super) fn counts(&self) -> BTreeMap<(String, &'static str), usize> {
        self.counts.clone()
    }
}
