//! Same/different full commit OIDs over actual Git objects and real Hub receipts.
//! This reuses the existing consumer fixture; no App click or producer execution
//! is claimed, and no verified-history/publication capability is constructed here.
use super::*;
use std::{cell::RefCell, fs, path::PathBuf};

type ActualRead = (PathBuf, String, &'static str);
thread_local! {
    static READS: RefCell<Option<Vec<ActualRead>>> = const { RefCell::new(None) };
}
pub(crate) fn record(catalog: &Catalog, revision: &str, kind: &'static str) {
    READS.with(|reads| {
        if let Some(reads) = reads.borrow_mut().as_mut() {
            reads.push((catalog.root.clone(), revision.to_owned(), kind));
        }
    });
}
struct Observe;
impl Observe {
    fn start() -> Self {
        READS.with(|reads| assert!(reads.borrow_mut().replace(vec![]).is_none()));
        Self
    }
    fn count(&self, catalog: &Catalog, revision: &str, kind: &str) -> usize {
        READS.with(|reads| {
            reads
                .borrow()
                .as_ref()
                .unwrap()
                .iter()
                .filter(|(root, oid, operation)| {
                    root == &catalog.root && oid == revision && *operation == kind
                })
                .count()
        })
    }
}
impl Drop for Observe {
    fn drop(&mut self) {
        READS.with(|reads| {
            reads.borrow_mut().take();
        });
    }
}

pub(crate) fn observe_hydration(
    catalog: &Catalog,
    history: &VerifiedRecoveryHistory,
    through: u64,
) -> Result<HydrationReadObservation, String> {
    let _observed = Observe::start();
    let (blocks, unavailable) = hydrate_publication_rechecks(catalog, history, through)?;
    // This is the existing physical tree/catalog read hook, aggregated across
    // every nested owner. A new owner cannot hide a duplicate physical read.
    let counts = READS.with(|reads| {
        let mut counts = BTreeMap::new();
        for (root, oid, kind) in reads.borrow().as_ref().unwrap() {
            if root == &catalog.root {
                *counts.entry((oid.clone(), *kind)).or_insert(0) += 1;
            }
        }
        counts
    });
    Ok(HydrationReadObservation {
        through_seq: through,
        blocks,
        unavailable,
        counts,
    })
}

// Insert a real same-tree parent between the frozen source and publication.
// Its distinct original preparation is actually appended/confirmed at that
// parent, then the same approved frozen files are committed on the new parent.
fn different_parent() -> PublicationFixture {
    let mut f = PublicationFixture::new();
    let metadata = read_publication_metadata(&f.catalog, &f.commit).unwrap();
    let source = metadata.manifest_proof.freeze.source_revision.clone();
    let source_tree = git(f.root.path(), &["rev-parse", &format!("{source}^{{tree}}")]);
    let parent = git(
        f.root.path(),
        &[
            "commit-tree",
            &source_tree,
            "-p",
            &source,
            "-m",
            "actual same-tree parent before approved publication",
        ],
    );
    assert_ne!(parent, source);
    let old_commit = f.commit.clone();
    let message = git(
        f.root.path(),
        &["show", "--no-patch", "--format=%B", &old_commit],
    );
    git(f.root.path(), &["reset", "--hard", &parent]);
    let mut preparation: PublicationPrepared =
        serde_json::from_value(metadata.manifest_proof.prepared_event.data.clone()).unwrap();
    preparation.authority_revision = parent.clone();
    let PublicationPrepareAction::PreparePublication {
        authority_revision, ..
    } = &mut preparation.request_body.action;
    *authority_revision = parent.clone();
    let request_id = uuid::Uuid::now_v7().to_string();
    let eid = request_event_eid(&request_id, "baseline.publication_prepared").unwrap();
    let hash = contract::canonical_hash(&preparation.request_body).unwrap();
    preparation.p3_request.request_id = request_id.clone();
    preparation.p3_request.body_hash = hash.clone();
    preparation.p3_receipt.request_id = request_id;
    preparation.p3_receipt.body_hash = hash;
    preparation.p3_receipt.result_ref.authority_revision = parent.clone();
    preparation.p3_receipt.result_ref.prepared_eid = eid.clone();
    let mut original = crate::recovery_outbox::original_of(&metadata.manifest_proof.prepared_event);
    original.eid = eid;
    original.data = serde_json::to_value(preparation).unwrap();
    let actual = append_fixture(&mut f.hub, &mut f.outbox, original);
    assert!(!actual.stale);
    git(f.root.path(), &["read-tree", "--reset", "-u", &old_commit]);
    let file = f.root.path().join(&f.manifest_path);
    let mut manifest = front(&fs::read_to_string(&file).unwrap()).unwrap();
    manifest["publication"]["prepared_event"] = json!(actual);
    manifest["x-local-read"] = json!({"future":[1,"preserved"]});
    fs::write(
        file,
        format!("---\n{}---\n", serde_yaml::to_string(&manifest).unwrap()),
    )
    .unwrap();
    git(f.root.path(), &["add", "."]);
    git(f.root.path(), &["commit", "-qm", &message]);
    f.commit = git(f.root.path(), &["rev-parse", "HEAD"]);
    git(
        f.root.path(),
        &[
            "tag",
            "-f",
            &metadata.manifest_proof.binding.target_baseline,
            &f.commit,
        ],
    );
    f.catalog = Catalog::load(f.root.path()).unwrap();
    assert_eq!(
        git(f.root.path(), &["rev-parse", &format!("{parent}^{{tree}}")]),
        source_tree
    );
    f
}
fn history(f: &PublicationFixture) -> VerifiedRecoveryHistory {
    read_recovery_history(&f.catalog, &f.outbox.read_inputs().unwrap()).unwrap()
}
fn loose(f: &PublicationFixture, oid: &str) -> PathBuf {
    let path = f
        .catalog
        .common_dir
        .join("objects")
        .join(&oid[..2])
        .join(&oid[2..]);
    assert!(
        path.is_file(),
        "fixture requires this actual loose object: {oid}"
    );
    path
}

#[test]
fn one_same_oid_operation_reads_each_actual_tree_and_catalog_once() {
    let mut f = PublicationFixture::new();
    let manifest_path = f.manifest_path.clone();
    amend_publication(&mut f, &manifest_path, |manifest| {
        manifest["x-local-read"] = json!({"future":[1,"preserved"]});
    });
    let metadata = read_publication_metadata(&f.catalog, &f.commit).unwrap();
    let parent = git(f.root.path(), &["rev-parse", &format!("{}^", f.commit)]);
    assert_eq!(metadata.manifest_proof.freeze.source_revision, parent);
    let verified = history(&f);
    for _ in 0..2 {
        // A second prove still performs fresh physical reads. This is not a
        // cross-call map of publication proofs or decoded immutable objects.
        let observed = Observe::start();
        let proof = prove_publication(&f.catalog, &verified, &f.commit).unwrap();
        assert_eq!(
            baseline_created_event(&proof).unwrap().data["target"],
            "towermd/B0"
        );
        for kind in ["tree", "catalog"] {
            assert_eq!(
                observed.count(&f.catalog, &parent, kind),
                1,
                "parent {kind}"
            );
            assert_eq!(
                observed.count(&f.catalog, &f.commit, kind),
                1,
                "candidate {kind}"
            );
        }
    }
    assert_eq!(
        read_publication_metadata(&f.catalog, &f.commit)
            .unwrap()
            .manifest["x-local-read"],
        json!({"future":[1,"preserved"]})
    );
    assert_eq!(
        f.catalog.objects["M"].get_str("x-user"),
        Some("\u{539f}\u{6587}")
    );
    // The public reader remains an independent physical read on every call.
    for _ in 0..2 {
        let observed = Observe::start();
        assert_eq!(
            publication_base_directory(&f.catalog, &parent).unwrap(),
            metadata.manifest_proof.freeze.base_directory
        );
        assert_eq!(observed.count(&f.catalog, &parent, "tree"), 1);
        assert_eq!(observed.count(&f.catalog, &parent, "catalog"), 1);
    }
}

#[test]
fn different_full_oids_with_identical_trees_keep_independent_source_read() {
    let f = different_parent();
    let metadata = read_publication_metadata(&f.catalog, &f.commit).unwrap();
    let source = &metadata.manifest_proof.freeze.source_revision;
    let parent = git(f.root.path(), &["rev-parse", &format!("{}^", f.commit)]);
    assert_ne!(source, &parent);
    assert_eq!(
        git(f.root.path(), &["rev-parse", &format!("{source}^{{tree}}")]),
        git(f.root.path(), &["rev-parse", &format!("{parent}^{{tree}}")])
    );
    let verified = history(&f);
    let observed = Observe::start();
    let proof = prove_publication(&f.catalog, &verified, &f.commit).unwrap();
    assert_eq!(
        baseline_created_event(&proof).unwrap().data["target"],
        "towermd/B0"
    );
    for oid in [source.as_str(), parent.as_str(), f.commit.as_str()] {
        for kind in ["tree", "catalog"] {
            assert_eq!(observed.count(&f.catalog, oid, kind), 1, "{oid} {kind}");
        }
    }
    drop(observed);
    // Remove the actual older source commit after a successful proof. The same
    // history cannot bypass original ancestry/source I/O on a subsequent prove.
    let object = loose(&f, source);
    let saved = object.with_extension("publication-withheld");
    fs::rename(&object, &saved).unwrap();
    let result = prove_publication(&f.catalog, &verified, &f.commit);
    fs::rename(&saved, &object).unwrap();
    assert_eq!(result.err().unwrap(), "cannot inspect proof ancestry");
    assert!(prove_publication(&f.catalog, &verified, &f.commit).is_ok());
}

#[test]
fn protected_non_markdown_blob_is_physically_checked_for_same_and_different_oids() {
    for different in [false, true] {
        let f = if different {
            different_parent()
        } else {
            PublicationFixture::new()
        };
        let metadata = read_publication_metadata(&f.catalog, &f.commit).unwrap();
        let freeze = &metadata.manifest_proof.freeze;
        let (protected_path, entry) = freeze
            .base_directory
            .iter()
            .find(|(path, _)| path.ends_with("/.gitkeep"))
            .expect("init fixture must contain a protected non-Markdown file");
        let (mode, oid) = entry.split_once(' ').unwrap();
        let verified = history(&f);
        let object = loose(&f, oid);
        let saved = object.with_extension("publication-withheld");
        fs::rename(&object, &saved).unwrap();
        let proof = prove_publication(&f.catalog, &verified, &f.commit);
        let independent = publication_base_directory(&f.catalog, &freeze.source_revision);
        fs::rename(&saved, &object).unwrap();
        for error in [proof.err().unwrap(), independent.err().unwrap()] {
            // Catalog's actual --long tree read may reject the missing blob's
            // BAD size before the later cat-file check. Match that exact row,
            // including its original mode, OID and path, not arbitrary I/O.
            let catalog_missing = error
                .strip_prefix(
                    "\u{65e0}\u{6cd5}\u{89e3}\u{6790} git ls-tree \u{8bb0}\u{5f55}\u{ff1a}",
                )
                .and_then(|record| serde_json::from_str::<String>(record).ok())
                .is_some_and(|record| {
                    record.split_once('\t').is_some_and(|(metadata, path)| {
                        path == protected_path.as_str()
                            && metadata.split_whitespace().eq([mode, "blob", oid, "BAD"])
                    })
                });
            assert!(
                error.contains(&format!("git cat-file -t {oid}")) || catalog_missing,
                "{error}"
            );
        }
        assert!(prove_publication(&f.catalog, &verified, &f.commit).is_ok());
        assert_eq!(
            publication_base_directory(&f.catalog, &freeze.source_revision).unwrap(),
            freeze.base_directory
        );
    }
}

#[test]
fn actual_main_advance_rejects_old_binding_and_fresh_history_still_proves_publication() {
    let mut f = different_parent();
    let verified = history(&f);
    assert!(prove_publication(&f.catalog, &verified, &f.commit).is_ok());
    let before = f.catalog.revision.clone();
    fs::write(
        f.root.path().join("unprotected-code.txt"),
        "actual main progress\n",
    )
    .unwrap();
    git(f.root.path(), &["add", "unprotected-code.txt"]);
    git(
        f.root.path(),
        &["commit", "-qm", "actual later main progress"],
    );
    assert_ne!(git(f.root.path(), &["rev-parse", "main"]), before);
    assert_eq!(
        prove_publication(&f.catalog, &verified, &f.commit)
            .err()
            .unwrap(),
        "history authority/project binding changed"
    );
    f.catalog = Catalog::load(f.root.path()).unwrap();
    assert_eq!(
        prove_publication(&f.catalog, &verified, &f.commit)
            .err()
            .unwrap(),
        "history authority/project binding changed"
    );
    let fresh = history(&f);
    let proof = prove_publication(&f.catalog, &fresh, &f.commit).unwrap();
    assert_eq!(
        baseline_created_event(&proof).unwrap().data["commit"],
        f.commit
    );
}

#[test]
fn finite_immutable_reads_keep_actual_tags_and_candidate_conflicts_live() {
    let f = PublicationFixture::new();
    let verified = history(&f);
    let metadata = read_publication_metadata(&f.catalog, &f.commit).unwrap();
    let mut reads = PublicationReads::new(&f.catalog).unwrap();
    for _ in 0..2 {
        let proof =
            prove_publication_with_reads(&f.catalog, &verified, &f.commit, &mut reads).unwrap();
        assert_eq!(
            baseline_created_event(&proof).unwrap().data["target"],
            "towermd/B0"
        );
        reads.probe(&f.catalog).unwrap();
    }
    let counts = reads.counts();
    for kind in ["tree", "catalog", "message", "parents"] {
        assert_eq!(
            counts.get(&(f.commit.clone(), kind)),
            Some(&1),
            "one physical immutable read per full OID/kind within this scope: {kind}"
        );
    }
    // A different HistoryCandidate cannot borrow the previous accepted role.
    // Immutable cache hits still reach insert_original on every verification.
    let mut conflicting = verified.candidate.clone();
    let mut changed = metadata.decision_proof.approval_event.clone();
    changed.data["injected"] = json!(true);
    conflicting.originals.insert(changed.eid.clone(), changed);
    assert!(check_publication_payload_with_reads(
        &f.catalog,
        &mut conflicting,
        &f.commit,
        &mut reads
    )
    .is_err());
    // A cached tree/blob/catalog never supplies the current exact tag.
    let target = metadata.manifest_proof.binding.target_baseline;
    git(f.root.path(), &["tag", "-d", &target]);
    assert!(prove_publication_with_reads(&f.catalog, &verified, &f.commit, &mut reads).is_err());
    assert!(reads.probe(&f.catalog).is_err());
    git(f.root.path(), &["tag", &target, &f.commit]);
    // The next independent operation has a new owner, not a cache grant.
    let mut next = PublicationReads::new(&f.catalog).unwrap();
    prove_publication_with_reads(&f.catalog, &verified, &f.commit, &mut next).unwrap();
    assert_eq!(next.counts().get(&(f.commit.clone(), "tree")), Some(&1));
}
