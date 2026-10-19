//! Initial-baseline publication through the existing repository writer.
//! Candidate inspection is deliberately distinct from a completed Git proof.
use super::{acquire_main, renew_fence, validate_authority_shared, SnapshotWriter};
use crate::{
    baseline_draft::FrozenDraft,
    catalog::Catalog,
    hub_protocol::{Event, Fence},
    project_session::ProjectSession,
    recovery_contract::{
        self,
        baseline::{ApprovalFact, ReviewBinding, Verdict},
        recorded::BaselineReviewRecorded,
    },
    recovery_git::{self, VerifiedRecoveryHistory},
    recovery_outbox::RecoveryOutbox,
    repository_writer::{self, RepositoryRef, Session},
    snapshot_service::{read_draft, SavedDraft},
};

use serde::Serialize;
use std::{
    collections::BTreeMap,
    fmt,
    path::Path,
    sync::{Mutex, TryLockError},
};

/// Only this writer can construct a preparation intent after actual draft and
/// original accepted-approval reads. It grants no lease or accepted receipt.
pub(crate) struct PublicationPreparation {
    approval: Event,
    fact: ApprovalFact,
    frozen: FrozenDraft,
    authority_revision: String,
    fence: Fence,
    host: String,
}
impl PublicationPreparation {
    pub(crate) fn approval_eid(&self) -> &str {
        &self.fact.approval_eid
    }
    pub(crate) fn binding(&self) -> &ReviewBinding {
        &self.fact.binding
    }
    pub(crate) fn authority_revision(&self) -> &str {
        &self.authority_revision
    }
    pub(crate) fn fence(&self) -> &Fence {
        &self.fence
    }
    pub(crate) fn host(&self) -> &str {
        &self.host
    }
    pub(crate) fn approval(&self) -> &Event {
        &self.approval
    }
}

fn load_approval<'a>(
    history: &'a VerifiedRecoveryHistory,
    approval_eid: &str,
) -> Result<(&'a Event, BaselineReviewRecorded), String> {
    let original = history
        .baseline_review_original(approval_eid)
        .ok_or("accepted baseline approval not found")?;
    if original.kind != "baseline.approved"
        || original.layer != "L1"
        || original.stale
        || !original.guards.is_empty()
    {
        return Err("publication requires an original nonstale approval without guards".into());
    }
    let recorded: BaselineReviewRecorded =
        serde_json::from_value(original.data.clone()).map_err(|e| e.to_string())?;
    recovery_contract::validate_recorded_data(
        &recorded.request_body.project_id,
        "baseline.approved",
        &original.data,
    )?;
    if recorded.approval.approval_eid != approval_eid
        || recorded.approval.verdict != Verdict::Approved
    {
        return Err("publication approval identity/verdict mismatch".into());
    }
    Ok((original, recorded))
}
fn checked_draft(catalog: &Catalog, fact: &ApprovalFact) -> Result<SavedDraft, String> {
    crate::baseline_current::baseline_number(&fact.binding.target_baseline)?;
    let saved = read_draft(catalog, &fact.binding.object_id)
        .map_err(|e| e.to_string())?
        .ok_or("approved draft ref not found")?;
    if crate::baseline_review::review_binding(saved.frozen(), saved.revision()) != fact.binding {
        return Err("approved binding differs from actual draft revision/content/target".into());
    }
    if crate::baseline_draft::freeze(catalog, &saved.frozen().bundle)? != *saved.frozen() {
        return Err("approved protected source changed".into());
    }
    Ok(saved)
}

fn capture_preparation(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    approval_eid: &str,
    fence: Fence,
    host: &str,
) -> Result<PublicationPreparation, String> {
    let (original, recorded) = load_approval(history, approval_eid)?;
    let saved = checked_draft(catalog, &recorded.approval)?;
    if recorded.approval.binding.target_baseline != "towermd/B0" {
        crate::recovery_git::publication_context_check(
            catalog,
            history,
            saved.frozen(),
            &recorded.approval.binding,
        )?;
    }
    Ok(PublicationPreparation {
        approval: original.clone(),
        fact: recorded.approval,
        frozen: saved.frozen().clone(),
        authority_revision: catalog.revision.clone(),
        fence,
        host: host.into(),
    })
}

struct Authority {
    catalog: Catalog,
    remote: Option<String>,
    tags: BTreeMap<String, String>,
}
fn local_tags(git: &Session, root: &Path) -> Result<BTreeMap<String, String>, String> {
    let listing = git.run(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(symref)",
            "refs/tags/towermd/",
        ],
        None,
    )?;
    let mut tags = BTreeMap::new();
    for line in listing.lines() {
        let fields: Vec<_> = line.split('\0').collect();
        if fields.len() != 3 || !fields[2].is_empty() {
            return Err("baseline tag is symbolic or malformed".into());
        }
        let label = fields[0]
            .strip_prefix("refs/tags/")
            .ok_or("invalid baseline tag ref")?;
        if !label.starts_with("towermd/B") {
            continue;
        }
        let number = label
            .strip_prefix("towermd/B")
            .ok_or("invalid baseline tag")?;
        let parsed = number
            .parse::<u64>()
            .map_err(|_| "invalid baseline tag number")?;
        if parsed.to_string() != number {
            return Err("noncanonical baseline tag number".into());
        }
        recovery_contract::object_id(fields[1])?;
        tags.insert(label.into(), fields[1].into());
    }
    Ok(tags)
}
fn has_origin(root: &Path) -> Result<bool, String> {
    // Session's defensive remote.origin.mirror=false is not a configured remote.
    Ok(repository_writer::read_git(root, &["remote"])?
        .lines()
        .any(|remote| remote == "origin"))
}
fn fresh_authority(original: &Catalog, git: &Session) -> Result<Authority, String> {
    let root = &original.root;
    let mut remote = None;
    let mut tags = local_tags(git, root)?;
    if has_origin(root)? {
        let url = git.origin_push_url(root)?;
        let prefix = format!("refs/towermd/publication-fetch/{}", uuid::Uuid::now_v7());
        let main_ref = format!("{prefix}/main");
        let tag_prefix = format!("{prefix}/tags/");
        let result = (|| {
            // A remote symbolic branch/tag must not be silently dereferenced by fetch.
            let advertised = git.run(
                root,
                &[
                    "ls-remote",
                    "--symref",
                    "--",
                    &url,
                    "refs/heads/main",
                    "refs/tags/towermd/*",
                ],
                None,
            )?;
            if advertised.lines().any(|line| line.starts_with("ref:")) {
                return Err("remote authority/tag is symbolic".into());
            }
            git.run(
                root,
                &[
                    "fetch",
                    "--no-tags",
                    "--no-recurse-submodules",
                    "--no-write-fetch-head",
                    "--no-auto-maintenance",
                    "--no-write-commit-graph",
                    "--no-prune",
                    "--refmap=",
                    "--",
                    &url,
                    &format!("refs/heads/main:{main_ref}"),
                    &format!("refs/tags/towermd/*:{tag_prefix}*"),
                ],
                None,
            )?;
            let revision = git.run(
                root,
                &["rev-parse", "--verify", &format!("{main_ref}^{{commit}}")],
                None,
            )?;
            let actual = git.run(
                root,
                &[
                    "for-each-ref",
                    "--format=%(refname)%00%(objectname)",
                    &tag_prefix,
                ],
                None,
            )?;
            let mut observed = BTreeMap::new();
            for line in actual.lines() {
                let (reference, oid) = line.split_once('\0').ok_or("invalid fetched tag")?;
                let suffix = reference
                    .strip_prefix(&tag_prefix)
                    .ok_or("wrong fetched tag namespace")?;
                if !suffix.starts_with('B') {
                    continue;
                }
                let label = format!("towermd/{suffix}");
                let number = suffix[1..]
                    .parse::<u64>()
                    .map_err(|_| "invalid remote baseline tag")?;
                if suffix != format!("B{number}") {
                    return Err("noncanonical remote baseline tag".into());
                }
                recovery_contract::object_id(oid)?;
                observed.insert(label, oid.to_owned());
            }
            for (label, old) in &tags {
                if observed.get(label) != Some(old) {
                    return Err(
                        "local and remote baseline tags differ; refusing to overwrite either"
                            .into(),
                    );
                }
            }
            let fetched_catalog = Catalog::at_revision(root, &revision)?
                .ok_or("remote main has no project configuration")?;
            if fetched_catalog.common_dir != original.common_dir
                || fetched_catalog.config.to_yaml() != original.config.to_yaml()
            {
                return Err("remote publication project/configuration changed".into());
            }
            // Install only tags actually observed at the publication remote.
            // The proof reader still verifies the entire actual publication.
            for (label, oid) in &observed {
                if tags.contains_key(label) {
                    continue;
                }
                let reference = format!("refs/tags/{label}");
                git.run(
                    root,
                    &[
                        "update-ref",
                        "--no-deref",
                        &reference,
                        oid,
                        &"0".repeat(oid.len()),
                    ],
                    None,
                )?;
            }
            tags = observed;
            let current = crate::catalog::authority_revision(root)?;
            if current != revision {
                git.cas_ref(root, &RepositoryRef::origin_main(), &revision, &current)?;
            }
            Ok::<(), String>(())
        })();
        let retained = git.run(
            root,
            &["for-each-ref", "--format=%(refname)", &prefix],
            None,
        )?;
        for reference in retained.lines() {
            git.run(root, &["update-ref", "--no-deref", "-d", reference], None)?;
        }
        result?;
        remote = Some(url);
    }
    let catalog = Catalog::load(root)?;
    if catalog.common_dir != original.common_dir
        || catalog.config.to_yaml() != original.config.to_yaml()
    {
        return Err("publication project/configuration changed".into());
    }
    Ok(Authority {
        catalog,
        remote,
        tags,
    })
}
fn require_next_target(authority: &Authority, target: &str) -> Result<(), String> {
    let number = crate::baseline_current::baseline_number(target)?;
    if authority.tags.len() as u64 != number {
        return Err("baseline target changed or tag chain has a gap".into());
    }
    for index in 0..number {
        let label = format!("towermd/B{index}");
        if !authority.tags.contains_key(&label) {
            return Err("baseline tag chain has a gap".into());
        }
    }
    Ok(())
}

fn checked_out_main(git: &Session, root: &Path) -> Result<Option<String>, String> {
    git.main_worktree(root)?.map(|path| {
        let text = path.to_str().ok_or("main worktree path is not UTF-8")?;
        let quoted = format!("'{}'", text.replace('\'', "'\\''"));
        Ok(format!("main is checked out at {text}; run git -C {quoted} switch -c <your-new-branch> yourself, then retry"))
    }).transpose()
}

#[cfg(test)]
#[path = "baseline_publish_tests.rs"]
mod tests;

fn materialize_candidate(
    git: &Session,
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    intent: &PublicationPreparation,
    prepared: &Event,
    verify: &mut impl FnMut() -> Result<(), String>,
) -> Result<recovery_git::PublicationCandidate, String> {
    let target = RepositoryRef::publication(intent.approval_eid())?;
    let old = repository_writer::direct_revision(&catalog.root, &target)?;
    if let Some(old) = &old {
        let candidate = recovery_git::read_publication_candidate(catalog, history, old)?;
        if candidate.approval_eid() != intent.approval_eid()
            || candidate.binding() != intent.binding()
        {
            return Err("retained candidate differs from original approval binding".into());
        }
        if candidate.prepared() == prepared && candidate.parent() == catalog.revision {
            verify()?;
            return Ok(candidate);
        }
        if is_ancestor(git, &catalog.root, old, &catalog.revision)? {
            return Err("already published candidate must be resumed, never replaced".into());
        }
    }
    let audit = recovery_git::publication_audit(
        catalog,
        &intent.frozen,
        &intent.approval,
        prepared,
        history,
    )?;
    let mut files: Vec<_> = intent
        .frozen
        .files
        .iter()
        .map(|(path, file)| repository_writer::TreeFile {
            path,
            mode: &file.mode,
            content: &file.text,
        })
        .collect();
    files.extend([
        repository_writer::TreeFile {
            path: &audit.decision_path,
            mode: "100644",
            content: &audit.decision,
        },
        repository_writer::TreeFile {
            path: &audit.manifest_path,
            mode: "100644",
            content: &audit.manifest,
        },
    ]);
    let tree = git.build_tree(
        &catalog.root,
        &catalog.revision,
        &files,
        &intent.frozen.bundle.deletions,
    )?;
    verify()?;
    let commit = git.commit_tree(&catalog.root, &tree, &[&catalog.revision], &audit.message)?;
    let candidate = recovery_git::read_publication_candidate(catalog, history, &commit)?;
    if candidate.binding() != intent.binding() {
        return Err("actual candidate binding mismatch".into());
    }
    verify()?;
    if repository_writer::direct_revision(&catalog.root, &target)? != old {
        return Err("publication candidate ref changed before CAS".into());
    }
    git.cas_ref(
        &catalog.root,
        &target,
        &commit,
        &old.unwrap_or_else(|| "0".repeat(commit.len())),
    )?;
    Ok(candidate)
}
fn is_ancestor(git: &Session, root: &Path, ancestor: &str, tip: &str) -> Result<bool, String> {
    let output = git.run_output(root, &["merge-base", "--is-ancestor", ancestor, tip])?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(format!(
            "cannot inspect publication ancestry: {}",
            String::from_utf8_lossy(&output.stderr)
        )),
    }
}
fn published_candidates(
    authority: &Authority,
    history: &VerifiedRecoveryHistory,
    git: &Session,
) -> Result<Vec<recovery_git::PublicationCandidate>, String> {
    let listing = git.run(
        &authority.catalog.root,
        &[
            "log",
            "--format=%H",
            "--fixed-strings",
            "--grep=Towermd-Publication:",
            &authority.catalog.revision,
        ],
        None,
    )?;
    let mut candidates = vec![];
    for commit in listing.lines() {
        let message = git.run(
            &authority.catalog.root,
            &["show", "--no-patch", "--format=%B", commit],
            None,
        )?;
        if matches!(
            recovery_git::parse_trailer_context(&message)?,
            recovery_git::TrailerContext::Publication(_)
        ) {
            candidates.push(recovery_git::read_publication_candidate(
                &authority.catalog,
                history,
                commit,
            )?);
        }
    }
    Ok(candidates)
}
fn push_failure(output: &std::process::Output, stage: &str) -> String {
    format!(
        "{stage} not confirmed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
struct MainFailure {
    reason: String,
    retryable: bool,
}
impl From<String> for MainFailure {
    fn from(reason: String) -> Self {
        Self {
            reason,
            retryable: false,
        }
    }
}
impl From<&str> for MainFailure {
    fn from(reason: &str) -> Self {
        Self::from(reason.to_owned())
    }
}
fn publish_main(
    git: &Session,
    authority: &Authority,
    candidate: &recovery_git::PublicationCandidate,
) -> Result<(), MainFailure> {
    let root = &authority.catalog.root;
    if let Some(url) = &authority.remote {
        boundary(root, "BeforeMainPush")?;
        let output = git.push_commit(root, url, candidate.commit(), &RepositoryRef::main())?;
        if !output.status.success() {
            return Err(MainFailure {
                reason: push_failure(&output, "main push"),
                retryable: matches!(
                    repository_writer::classify_push(
                        &String::from_utf8_lossy(&output.stdout),
                        &String::from_utf8_lossy(&output.stderr)
                    ),
                    repository_writer::PushFailure::NonFastForward
                ),
            });
        }
    } else {
        if let Some(reason) = checked_out_main(git, root)? {
            return Err(MainFailure::from(reason));
        }
        let actual = repository_writer::direct_revision(root, &RepositoryRef::main())?
            .ok_or("main is missing")?;
        if actual != candidate.parent() {
            return Err(MainFailure {
                reason: "main changed before publication CAS".into(),
                retryable: true,
            });
        }
        git.cas_ref_checked(root, &RepositoryRef::main(), candidate.commit(), &actual)
            .map_err(|failure| match failure {
                repository_writer::CasRefFailure::Conflict { reason } => MainFailure {
                    reason,
                    retryable: true,
                },
                repository_writer::CasRefFailure::Uncertain { reason } => MainFailure {
                    reason,
                    retryable: false,
                },
            })?;
    }
    Ok(())
}
fn publish_tag(
    git: &Session,
    authority: &Authority,
    candidate: &recovery_git::PublicationCandidate,
) -> Result<(), String> {
    let root = &authority.catalog.root;
    if !is_ancestor(git, root, candidate.commit(), &authority.catalog.revision)? {
        return Err("cannot tag a candidate outside authoritative main".into());
    }
    let target = &candidate.binding().target_baseline;
    if let Some(oid) = authority.tags.get(target) {
        let peeled = git.run(
            root,
            &["rev-parse", "--verify", &format!("{oid}^{{commit}}")],
            None,
        )?;
        if peeled != candidate.commit() {
            return Err("baseline tag already names another publication".into());
        }
        return Ok(());
    }
    require_next_target(authority, target)?;
    if let Some(url) = &authority.remote {
        // Publish the precise OID before installing a local tag: a rejected
        // remote push must not activate B0 in local authority readers.
        let output = git.push_commit(
            root,
            url,
            candidate.commit(),
            &RepositoryRef::baseline(target)?,
        )?;
        if !output.status.success() {
            return Err(push_failure(&output, "baseline tag push"));
        }
    } else {
        git.cas_ref(
            root,
            &RepositoryRef::baseline(target)?,
            candidate.commit(),
            &"0".repeat(candidate.commit().len()),
        )?;
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PublicationReport {
    Pending {
        publication_id: String,
        target: String,
        commit: Option<String>,
        stage: PublicationStage,
        reason: String,
    },
    Recorded {
        created: Box<recovery_contract::recorded::BaselineCreated>,
    },
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStage {
    Preparation,
    Main,
    Tag,
    Record,
}
#[derive(Clone, Debug)]
pub enum PublicationError {
    Busy,
    Conflict { reason: String },
    Refused { reason: String },
}
impl fmt::Display for PublicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => f.write_str("repository writer is busy"),
            Self::Refused { reason } | Self::Conflict { reason } => f.write_str(reason),
        }
    }
}
impl std::error::Error for PublicationError {}
impl From<String> for PublicationError {
    fn from(reason: String) -> Self {
        Self::Refused { reason }
    }
}
fn pending(
    eid: &str,
    target: &str,
    commit: Option<&str>,
    stage: PublicationStage,
    reason: impl Into<String>,
) -> PublicationReport {
    PublicationReport::Pending {
        publication_id: eid.into(),
        target: target.into(),
        commit: commit.map(str::to_owned),
        stage,
        reason: reason.into(),
    }
}
fn renew_publication(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    host: &str,
    fence: &Fence,
    now: u64,
) -> Result<(), String> {
    validate_authority_shared(session, catalog)?;
    let mut live = session.lock().map_err(|_| "session lock poisoned")?;
    live.bind_catalog(catalog)?;
    renew_fence(&mut live, host, fence, now)
}
fn recheck_intent(
    session: &Mutex<ProjectSession>,
    catalog: &Catalog,
    intent: &PublicationPreparation,
    now: u64,
) -> Result<(), String> {
    if crate::catalog::authority_revision(&catalog.root)? != intent.authority_revision {
        return Err("publication authority changed".into());
    }
    let saved = checked_draft(catalog, &intent.fact)?;
    if saved.frozen() != &intent.frozen {
        return Err("actual approved draft changed before publication".into());
    }
    let authority = Authority {
        catalog: Catalog::load(&catalog.root)?,
        remote: None,
        tags: local_tags(&Session::for_network(&catalog.root)?, &catalog.root)?,
    };
    require_next_target(&authority, &intent.fact.binding.target_baseline)?;
    renew_publication(session, catalog, &intent.host, &intent.fence, now)
}
fn boundary(root: &Path, point: &'static str) -> Result<(), String> {
    #[cfg(test)]
    {
        test_hooks::run(root, point)
    }
    #[cfg(not(test))]
    {
        let _ = (root, point);
        Ok(())
    }
}
// The production adapter only forwards these two typed operations to Session's
// durable producer bridges. This private trait is never an authorization API.
trait PublicationDurability {
    fn inputs(&self) -> Result<crate::recovery_outbox::RecoveryInputs, String>;
    fn sync(&mut self, catalog: &Catalog, now: u64) -> Result<(), String>;
    fn prepare(
        &mut self,
        catalog: &Catalog,
        intent: &PublicationPreparation,
        now: u64,
    ) -> Result<Event, String>;
    fn record(
        &mut self,
        catalog: &Catalog,
        proof: &recovery_git::VerifiedPublication,
        now: u64,
    ) -> Result<recovery_contract::recorded::BaselineCreated, String>;
    fn record_with_clock(
        &mut self,
        catalog: &Catalog,
        proof: &recovery_git::VerifiedPublication,
        clock: &mut dyn FnMut() -> Result<u64, String>,
    ) -> Result<recovery_contract::recorded::BaselineCreated, String> {
        self.record(catalog, proof, clock()?)
    }
}
fn run_publication(
    session: &Mutex<ProjectSession>,
    original: &Catalog,
    approval_eid: &str,
    host: &str,
    now: &mut impl FnMut() -> Result<u64, String>,
    durable: &mut impl PublicationDurability,
) -> Result<PublicationReport, PublicationError> {
    RepositoryRef::publication(approval_eid)?;
    let network = Session::for_network(&original.root)?;
    for attempt in 0..3 {
        let authority = fresh_authority(original, &network)?;
        durable.sync(&authority.catalog, now()?)?;
        let history = recovery_git::read_recovery_history(&authority.catalog, &durable.inputs()?)?;
        let published = published_candidates(&authority, &history, &network)?;
        let matching: Vec<_> = published
            .iter()
            .filter(|c| c.approval_eid() == approval_eid)
            .collect();
        if matching.len() > 1 {
            return Err("multiple main commits claim the same publication"
                .to_owned()
                .into());
        }
        if let Some(candidate) = matching.first() {
            return Ok(resume_publication(
                session,
                &authority.catalog,
                approval_eid,
                candidate.commit(),
                host,
                now,
                durable,
            ));
        }
        if let Some(other) = published
            .iter()
            .find(|c| !authority.tags.contains_key(&c.binding().target_baseline))
        {
            return Err(format!(
                "pending publication {} must be resumed before another baseline",
                other.approval_eid()
            )
            .into());
        }
        // A receipt or an unrelated tag cannot silently choose another target.
        let (_, recorded) = load_approval(&history, approval_eid)?;
        let target = recorded.approval.binding.target_baseline.clone();
        checked_draft(&authority.catalog, &recorded.approval)
            .map_err(|reason| PublicationError::Conflict { reason })?;
        require_next_target(&authority, &recorded.approval.binding.target_baseline)
            .map_err(|reason| PublicationError::Conflict { reason })?;
        if authority.remote.is_none() {
            if let Some(reason) = checked_out_main(&network, &original.root)? {
                return Ok(pending(
                    approval_eid,
                    &target,
                    None,
                    PublicationStage::Main,
                    reason,
                ));
            }
        }
        validate_authority_shared(session, &authority.catalog)?;
        let fence = {
            let mut live = session
                .lock()
                .map_err(|_| "session lock poisoned".to_owned())?;
            live.bind_catalog(&authority.catalog)?;
            acquire_main(&mut live, host, now()?)?
        };
        let intent = capture_preparation(&authority.catalog, &history, approval_eid, fence, host)?;
        recheck_intent(session, &authority.catalog, &intent, now()?)?;
        let prepared = match durable.prepare(&authority.catalog, &intent, now()?) {
            Ok(event) => event,
            Err(reason) => {
                return Ok(pending(
                    approval_eid,
                    &target,
                    None,
                    PublicationStage::Preparation,
                    reason,
                ))
            }
        };
        if let Err(reason) = boundary(&authority.catalog.root, "BeforeCandidate") {
            return Ok(pending(
                approval_eid,
                &target,
                None,
                PublicationStage::Preparation,
                reason,
            ));
        }
        let git = match Session::for_publication(&authority.catalog.root) {
            Ok(git) => git,
            Err(reason) => {
                return Ok(pending(
                    approval_eid,
                    &target,
                    None,
                    PublicationStage::Preparation,
                    reason,
                ))
            }
        };
        let mut verify = || {
            let observed = fresh_authority(original, &network)?;
            if observed.catalog.revision != intent.authority_revision {
                return Err("publication authority changed during candidate preparation".into());
            }
            recheck_intent(session, &observed.catalog, &intent, now()?)
        };
        let candidate = match materialize_candidate(
            &git,
            &authority.catalog,
            &history,
            &intent,
            &prepared,
            &mut verify,
        ) {
            Ok(candidate) => candidate,
            Err(reason) => {
                return Ok(pending(
                    approval_eid,
                    &target,
                    None,
                    PublicationStage::Preparation,
                    reason,
                ))
            }
        };
        let commit = candidate.commit();
        if let Err(reason) = boundary(&authority.catalog.root, "BeforePublish") {
            return Ok(pending(
                approval_eid,
                &target,
                Some(commit),
                PublicationStage::Main,
                reason,
            ));
        }
        let fresh = match fresh_authority(original, &network) {
            Ok(authority) => authority,
            Err(reason) => {
                return Ok(pending(
                    approval_eid,
                    &target,
                    Some(commit),
                    PublicationStage::Main,
                    reason,
                ))
            }
        };
        let already_on_main =
            match is_ancestor(&network, &original.root, commit, &fresh.catalog.revision) {
                Ok(value) => value,
                Err(reason) => {
                    return Ok(pending(
                        approval_eid,
                        &target,
                        Some(commit),
                        PublicationStage::Main,
                        reason,
                    ))
                }
            };
        if already_on_main {
            return Ok(resume_publication(
                session,
                &fresh.catalog,
                approval_eid,
                commit,
                host,
                now,
                durable,
            ));
        }
        if fresh.catalog.revision != candidate.parent() {
            if attempt < 2 {
                continue;
            }
            return Ok(pending(
                approval_eid,
                &target,
                Some(commit),
                PublicationStage::Main,
                "main changed across three checked attempts",
            ));
        }
        if let Err(reason) =
            now().and_then(|time| recheck_intent(session, &fresh.catalog, &intent, time))
        {
            return Ok(pending(
                approval_eid,
                &target,
                Some(commit),
                PublicationStage::Main,
                reason,
            ));
        }
        let retained = repository_writer::direct_revision(
            &original.root,
            &RepositoryRef::publication(approval_eid)?,
        );
        if retained
            .as_ref()
            .ok()
            .and_then(|revision| revision.as_deref())
            != Some(commit)
        {
            return Ok(pending(
                approval_eid,
                &target,
                Some(commit),
                PublicationStage::Main,
                retained.err().unwrap_or_else(|| {
                    "publication candidate ref changed before main update".into()
                }),
            ));
        }
        let published = publish_main(&git, &fresh, &candidate);
        if let Err(failure) = published {
            if let Ok(observed) = fresh_authority(original, &network) {
                match is_ancestor(&network, &original.root, commit, &observed.catalog.revision) {
                    Ok(true) => {
                        return Ok(resume_publication(
                            session,
                            &observed.catalog,
                            approval_eid,
                            commit,
                            host,
                            now,
                            durable,
                        ))
                    }
                    Ok(false)
                        if failure.retryable
                            && attempt < 2
                            && observed.catalog.revision != candidate.parent() =>
                    {
                        continue
                    }
                    Ok(false) => {}
                    Err(reason) => {
                        return Ok(pending(
                            approval_eid,
                            &target,
                            Some(commit),
                            PublicationStage::Main,
                            reason,
                        ))
                    }
                }
            }
            return Ok(pending(
                approval_eid,
                &target,
                Some(commit),
                PublicationStage::Main,
                failure.reason,
            ));
        }
        if let Err(reason) = boundary(&original.root, "AfterPublish") {
            return Ok(pending(
                approval_eid,
                &target,
                Some(commit),
                PublicationStage::Tag,
                reason,
            ));
        }
        return Ok(resume_publication(
            session,
            original,
            approval_eid,
            commit,
            host,
            now,
            durable,
        ));
    }
    unreachable!("the third attempt always returns")
}
fn resume_publication(
    session: &Mutex<ProjectSession>,
    original: &Catalog,
    approval_eid: &str,
    commit: &str,
    host: &str,
    now: &mut impl FnMut() -> Result<u64, String>,
    durable: &mut impl PublicationDurability,
) -> PublicationReport {
    let mut target = "towermd/B0".to_owned();
    let mut stage = PublicationStage::Tag;
    let result = (|| {
        let git = Session::for_network(&original.root)?;
        let authority = fresh_authority(original, &git)?;
        durable.sync(&authority.catalog, now()?)?;
        let history = recovery_git::read_recovery_history(&authority.catalog, &durable.inputs()?)?;
        let candidate =
            recovery_git::read_publication_candidate(&authority.catalog, &history, commit)?;
        target = candidate.binding().target_baseline.clone();
        if candidate.approval_eid() != approval_eid
            || !is_ancestor(&git, &original.root, commit, &authority.catalog.revision)?
        {
            return Err("original publication is not on authoritative main".into());
        }
        validate_authority_shared(session, &authority.catalog)?;
        let fence = {
            let mut live = session.lock().map_err(|_| "session lock poisoned")?;
            live.bind_catalog(&authority.catalog)?;
            acquire_main(&mut live, host, now()?)?
        };
        boundary(&original.root, "BeforeTag")?;
        renew_publication(session, &authority.catalog, host, &fence, now()?)?;
        publish_tag(&git, &authority, &candidate)?;
        let tagged = fresh_authority(original, &git)?;
        let history = recovery_git::read_recovery_history(&tagged.catalog, &durable.inputs()?)?;
        let proof = recovery_git::prove_publication(&tagged.catalog, &history, commit)?;
        stage = PublicationStage::Record;
        boundary(&original.root, "AfterTag")?;
        boundary(&original.root, "BeforeRecorded")?;
        durable.record_with_clock(&tagged.catalog, &proof, now)
    })();
    match result {
        Ok(created) => PublicationReport::Recorded {
            created: Box::new(created),
        },
        Err(reason) => pending(approval_eid, &target, Some(commit), stage, reason),
    }
}

/// Publication is incomplete until every exact S-population marker has its
/// original accepted body. A missing Q remains pending, never a substitute
/// population inferred from current Task projections.
fn publication_rechecks_complete(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    proof: &recovery_git::VerifiedPublication,
    created: &Event,
) -> Result<bool, String> {
    if created.data["target"] == "towermd/B0" {
        return Ok(true);
    }
    let plan =
        recovery_git::publication_recheck_plan_for_created(catalog, history, proof, created)?;
    let markers = history.publication_recheck_originals(catalog)?;
    for fact in &plan.facts {
        let expected = crate::proposals::required_event(
            catalog.config.project_id(),
            fact,
            &plan.source_commit,
            created,
        )?;
        let Some(actual) = markers.iter().copied().find(|actual| {
            !actual.stale
                && actual.seq > created.seq
                && crate::recovery_outbox::original_of(actual) == expected
        }) else {
            return Ok(false);
        };
        let local_attempt = inputs
            .pending()
            .iter()
            .any(|event| event.eid == expected.eid)
            || inputs
                .confirmed()
                .iter()
                .any(|event| event.eid == expected.eid);
        if local_attempt && !inputs.receipt_confirmed(actual) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Rebuildable selectors for the daemon's retry loop. These IDs carry no
/// authorization: publish_baseline rereads actual history, refs and live lease.
/// This reader never writes refs, objects, indices, receipts, or hub state.
pub fn pending_baseline_publications(
    catalog: &Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
) -> Result<Vec<String>, String> {
    crate::recovery_outbox::RecoveryContext::from_catalog(catalog)?;
    if crate::catalog::authority_revision(&catalog.root)? != catalog.revision {
        return Err("publication selectors require a current Catalog".into());
    }
    let history = recovery_git::read_recovery_history(catalog, inputs)?;
    let git = Session::for_network(&catalog.root)?;
    let current = Catalog::load(&catalog.root)?;
    let authority = Authority {
        catalog: current,
        remote: None,
        tags: local_tags(&git, &catalog.root)?,
    };
    let mut pending = std::collections::BTreeSet::new();
    let mut published = std::collections::BTreeSet::new();
    for candidate in published_candidates(&authority, &history, &git)? {
        let eid = candidate.approval_eid().to_owned();
        published.insert(eid.clone());
        let completed = recovery_git::prove_publication(catalog, &history, candidate.commit())
            .ok()
            .and_then(|proof| {
                recovery_git::baseline_created_event(&proof)
                    .ok()
                    .map(|canonical| (proof, canonical))
            })
            .is_some_and(|(proof, canonical)| {
                history
                    .publication_created_original(&canonical.eid)
                    .is_some_and(|actual| {
                        let local_attempt = inputs
                            .pending()
                            .iter()
                            .any(|event| event.eid == canonical.eid)
                            || inputs
                                .confirmed()
                                .iter()
                                .any(|event| event.eid == canonical.eid);
                        crate::recovery_outbox::original_of(actual) == canonical
                            && (!local_attempt || inputs.receipt_confirmed(actual))
                            && publication_rechecks_complete(
                                catalog, &history, inputs, &proof, actual,
                            )
                            .unwrap_or(false)
                    })
            });
        if !completed {
            pending.insert(eid);
        }
    }
    for event in history.baseline_request_originals(catalog)? {
        if event.kind != "baseline.approved" || published.contains(&event.eid) {
            continue;
        }
        let Ok((_, recorded)) = load_approval(&history, &event.eid) else {
            continue;
        };
        if checked_draft(catalog, &recorded.approval).is_ok() {
            pending.insert(event.eid.clone());
        }
    }
    // Retained private candidates are checked, not trusted as approvals. The
    // accepted original above remains mandatory before unpublished continuation.
    let listing = git.run(
        &catalog.root,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(symref)%00%(objecttype)",
            "refs/towermd/drafts/publication-*",
        ],
        None,
    )?;
    for line in listing.lines() {
        let row: Vec<_> = line.split('\0').collect();
        if row.len() != 4 || !row[2].is_empty() || row[3] != "commit" {
            return Err("unsafe publication candidate ref".into());
        }
        let candidate = recovery_git::read_publication_candidate(catalog, &history, row[1])?;
        if RepositoryRef::publication(candidate.approval_eid())?.name() != row[0] {
            return Err("publication candidate path encodes another identity".into());
        }
        if !published.contains(candidate.approval_eid()) {
            if let Ok((_, recorded)) = load_approval(&history, candidate.approval_eid()) {
                if candidate.binding() == &recorded.approval.binding
                    && checked_draft(catalog, &recorded.approval).is_ok()
                {
                    pending.insert(candidate.approval_eid().into());
                }
            }
        }
    }
    if crate::catalog::authority_revision(&catalog.root)? != catalog.revision
        || local_tags(&git, &catalog.root)? != authority.tags
        || git.run(
            &catalog.root,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(symref)%00%(objecttype)",
                "refs/towermd/drafts/publication-*",
            ],
            None,
        )? != listing
    {
        return Err("publication sources changed while selecting retries".into());
    }
    Ok(pending.into_iter().collect())
}

struct SessionDurability<'a, 'w> {
    writer: &'a SnapshotWriter,
    writing: &'a std::sync::MutexGuard<'w, ()>,
    host: &'a str,
    session: &'a Mutex<ProjectSession>,
    outbox: &'a mut RecoveryOutbox,
}
impl PublicationDurability for SessionDurability<'_, '_> {
    fn inputs(&self) -> Result<crate::recovery_outbox::RecoveryInputs, String> {
        self.outbox.read_inputs()
    }
    fn sync(&mut self, catalog: &Catalog, now: u64) -> Result<(), String> {
        ProjectSession::sync_shared(self.session, catalog, &self.outbox.read_inputs()?, now)
            .map_err(|error| error.to_string())
    }
    fn prepare(
        &mut self,
        catalog: &Catalog,
        intent: &PublicationPreparation,
        now: u64,
    ) -> Result<Event, String> {
        ProjectSession::prepare_publication_shared(self.session, catalog, self.outbox, intent, now)
            .map(|recorded| recorded.original().clone())
    }
    fn record(
        &mut self,
        catalog: &Catalog,
        proof: &recovery_git::VerifiedPublication,
        now: u64,
    ) -> Result<recovery_contract::recorded::BaselineCreated, String> {
        ProjectSession::record_publication_shared(self.session, catalog, self.outbox, proof, now)
    }
    fn record_with_clock(
        &mut self,
        catalog: &Catalog,
        proof: &recovery_git::VerifiedPublication,
        clock: &mut dyn FnMut() -> Result<u64, String>,
    ) -> Result<recovery_contract::recorded::BaselineCreated, String> {
        let canonical = recovery_git::baseline_created_event(proof)?;
        if canonical.data["target"] == "towermd/B0" {
            return self.record(catalog, proof, clock()?);
        }
        let history = recovery_git::read_recovery_history(catalog, &self.outbox.read_inputs()?)?;
        let actual = match history.publication_created_original(&canonical.eid) {
            Some(actual) => Some(actual.clone()),
            None => ProjectSession::publication_actual_shared(
                self.session,
                catalog,
                &self.outbox.read_inputs()?,
                &canonical,
                clock()?,
            )?,
        };
        if let Some(actual) = actual {
            let plan = recovery_git::publication_recheck_plan_for_created(
                catalog, &history, proof, &actual,
            )?;
            let result = ProjectSession::record_publication_shared(
                self.session,
                catalog,
                self.outbox,
                proof,
                clock()?,
            )?;
            ProjectSession::record_publication_rechecks_shared(
                self.session,
                catalog,
                self.outbox,
                &plan,
                &actual,
                clock()?,
            )?;
            return Ok(result);
        }
        let mut recorded = None;
        let mut complete = |commit: &str,
                            snapshot: &crate::snapshot::Snapshot,
                            at: u64,
                            outbox: &mut RecoveryOutbox| {
            let current = Catalog::load(&catalog.root)?;
            let plan = recovery_git::publication_recheck_plan(&current, proof, commit)?;
            if plan.through_seq != snapshot.through_seq {
                return Err("publication Q changed after durable snapshot".into());
            }
            let result = ProjectSession::record_publication_at_prefix_shared(
                self.session,
                &current,
                outbox,
                proof,
                Some(plan.through_seq),
                at,
            )?;
            let actual = outbox
                .read_inputs()?
                .confirmed()
                .iter()
                .find(|event| event.eid == canonical.eid)
                .cloned()
                .ok_or("publication creation lacks its durable actual S receipt")?;
            boundary(&catalog.root, "AfterCreatedBeforeMarkers")?;
            ProjectSession::record_publication_rechecks_shared(
                self.session,
                &current,
                outbox,
                &plan,
                &actual,
                at,
            )?;
            recorded = Some(result);
            Ok(())
        };
        let report = self.writer.write_publication_checkpoint(
            self.writing,
            self.session,
            catalog,
            self.host,
            clock,
            self.outbox,
            None,
            Some(&mut complete),
        )?;
        recorded.ok_or_else(|| {
            format!(
                "publication population snapshot incomplete: {}",
                report.status
            )
        })
    }
}
impl SnapshotWriter {
    /// Continue an exact accepted App approval. Git progress and this internal
    /// creation receipt never rewrite the original human review response.
    pub fn publish_baseline(
        &self,
        session: &Mutex<ProjectSession>,
        catalog: &Catalog,
        outbox: &mut RecoveryOutbox,
        approval_eid: &str,
        host: &str,
        mut now: impl FnMut() -> Result<u64, String>,
    ) -> Result<PublicationReport, PublicationError> {
        let _writing = match self.writing.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Err(PublicationError::Busy),
            Err(TryLockError::Poisoned(_)) => {
                return Err("repository writer lock poisoned".to_owned().into())
            }
        };
        crate::hub::validate_token("host", host).map_err(|error| PublicationError::Refused {
            reason: error.to_string(),
        })?;
        let inputs = outbox.read_inputs()?;
        let history = recovery_git::read_recovery_history(catalog, &inputs)?;
        if let Ok((_, approval)) = load_approval(&history, approval_eid) {
            if approval.approval.binding.target_baseline != "towermd/B0"
                && !local_tags(&Session::for_network(&catalog.root)?, &catalog.root)?
                    .contains_key(&approval.approval.binding.target_baseline)
            {
                let report = self.write_publication_checkpoint(
                    &_writing, session, catalog, host, &mut now, outbox, None, None,
                )?;
                if !matches!(report.status.as_str(), "written" | "unchanged") {
                    return Ok(pending(
                        approval_eid,
                        &approval.approval.binding.target_baseline,
                        report.commit.as_deref(),
                        PublicationStage::Preparation,
                        "Proposal must be durably snapshotted before publication",
                    ));
                }
            }
        }
        let mut durable = SessionDurability {
            writer: self,
            writing: &_writing,
            host,
            session,
            outbox,
        };
        run_publication(session, catalog, approval_eid, host, &mut now, &mut durable)
    }
}

#[cfg(test)]
pub(super) fn set_test_boundary(
    root: &Path,
    point: &'static str,
    hook: Box<dyn FnOnce() -> Result<(), String> + Send>,
) {
    test_hooks::set(root, point, hook);
}

#[cfg(test)]
mod test_hooks {
    use super::*;
    type Hook = Box<dyn FnOnce() -> Result<(), String> + Send>;
    type Hooks = BTreeMap<(std::path::PathBuf, &'static str), Hook>;
    static HOOKS: std::sync::OnceLock<Mutex<Hooks>> = std::sync::OnceLock::new();
    pub(super) fn set(root: &Path, point: &'static str, hook: Hook) {
        HOOKS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert((root.canonicalize().unwrap(), point), hook);
    }
    pub(super) fn run(root: &Path, point: &'static str) -> Result<(), String> {
        let hook = HOOKS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .remove(&(root.canonicalize().map_err(|e| e.to_string())?, point));
        match hook {
            Some(hook) => hook(),
            None => Ok(()),
        }
    }
}
