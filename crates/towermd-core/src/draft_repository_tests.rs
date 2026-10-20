use super::*;
use crate::{
    baseline_draft::{DraftBundle, DraftObject},
    recovery_outbox::RecoveryInputs,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
const NOW: u64 = 1_800_000_000_000;
fn git(root: &Path, args: &[&str]) -> String {
    crate::test_support::git(root, args)
}
struct Fixture {
    dir: tempfile::TempDir,
    catalog: Catalog,
    session: Mutex<ProjectSession>,
    frozen: FrozenDraft,
}
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-qb", "main"]);
    crate::init::init(root, &crate::init::InitOptions { docs_dir: None }).unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/main.py"), "# source\n").unwrap();
    fs::write(root.join("README.md"), "ordinary root document\n").unwrap();
    fs::write(
        root.join("docs/tower/ordinary.md"),
        "ordinary docs document\n",
    )
    .unwrap();
    let text = "---\r\ntype: Module\r\nid: A\r\ntitle: Original\r\npaths: ['src/**']\r\nverify: []\r\nx-future: {values: [2, 1]}\r\n---\r\nmodule bytes\r\n";
    fs::write(root.join("docs/tower/arch/A.md"), text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            root.join("docs/tower/arch/A.md"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let canvas = json!({"x-future":true,"nodes":[{"id":"A","type":"file","file":"docs/tower/arch/A.md","towermd":{"obj":"A"},"x":40,"y":20,"width":320,"height":180}],"edges":[]}).to_string();
    fs::write(root.join("docs/tower/tower.canvas"), &canvas).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "draft source"]);
    let catalog = Catalog::load(root).unwrap();
    let bundle = DraftBundle {
        id: "draft-1".into(),
        project_id: catalog.config.project_id().into(),
        docs_dir: catalog.config.docs_dir().into(),
        source_revision: catalog.revision.clone(),
        target_baseline: "towermd/B0".into(),
        objects: BTreeMap::from([(
            "A".into(),
            DraftObject {
                id: "A".into(),
                path: "docs/tower/arch/A.md".into(),
                text: text.replace("Original", "Human edit"),
            },
        )]),
        canvas_text: canvas,
        deletions: vec![],
        questions: vec!["review".into()],
        risks: vec![],
    };
    let frozen = freeze(&catalog, &bundle).unwrap();
    let inputs = RecoveryInputs::read(&root.join("unused-outbox"), &catalog).unwrap();
    let session = Mutex::new(
        ProjectSession::open(&catalog.common_dir.join("towermd"), &catalog, &inputs, NOW).unwrap(),
    );
    Fixture {
        dir,
        catalog,
        session,
        frozen,
    }
}
fn request_id() -> String {
    uuid::Uuid::now_v7().to_string()
}
fn prepared(frozen: &FrozenDraft, expected: Option<&str>, id: &str) -> PreparedDraftSaveRequest {
    let action = if let Some(expected) = expected {
        DraftSaveAction::EditDraft {
            id: frozen.bundle.id.clone(),
            expected_revision: expected.into(),
            edit: crate::baseline_draft::DraftEdit::Rename {
                object: "A".into(),
                title: "Human edit".into(),
            },
        }
    } else {
        let prefix = format!("{}/", frozen.bundle.docs_dir);
        let modules = frozen
            .bundle
            .objects
            .values()
            .cloned()
            .map(|mut object| {
                object.path = object.path.strip_prefix(&prefix).unwrap().to_owned();
                object
            })
            .collect();
        DraftSaveAction::SubmitBaseline {
            submit: crate::baseline_schema::BaselineSubmit {
                scan_id: frozen.bundle.id.clone(),
                project_id: frozen.bundle.project_id.clone(),
                source_revision: frozen.bundle.source_revision.clone(),
                intents: vec![],
                modules,
                interfaces: vec![],
                acceptances: vec![],
                questions: frozen.bundle.questions.clone(),
                risks: frozen.bundle.risks.clone(),
            },
        }
    };
    PreparedDraftSaveRequest::prepare(
        RequestBody {
            project_id: frozen.bundle.project_id.clone(),
            host: "host".into(),
            actor: if expected.is_some() {
                "human:test"
            } else {
                "process:towermd-cli"
            }
            .into(),
            run: None,
            action,
        },
        id,
    )
    .unwrap()
}
fn save(
    writer: &SnapshotWriter,
    fixture: &Fixture,
    frozen: &FrozenDraft,
    expected: Option<&str>,
    request: &str,
) -> Result<DraftMaterialization, DraftSaveError> {
    writer.save_draft(
        &fixture.session,
        &fixture.catalog,
        frozen,
        &prepared(frozen, expected, request),
        || Ok(NOW),
    )
}
struct UserState {
    paths: Vec<PathBuf>,
    states: Vec<(String, String, Vec<u8>)>,
    heads: String,
}
fn user_state(fixture: &Fixture, paths: Vec<PathBuf>) -> UserState {
    let states = paths
        .iter()
        .map(|path| {
            let index = PathBuf::from(git(
                path,
                &["rev-parse", "--path-format=absolute", "--git-path", "index"],
            ));
            (
                git(path, &["rev-parse", "HEAD"]),
                git(path, &["status", "--porcelain=v1"]),
                fs::read(index).unwrap(),
            )
        })
        .collect();
    UserState {
        paths,
        states,
        heads: git(
            fixture.dir.path(),
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads/",
            ],
        ),
    }
}
fn assert_user_state(fixture: &Fixture, before: &UserState) {
    let after = user_state(fixture, before.paths.clone());
    assert_eq!(before.states, after.states);
    assert_eq!(before.heads, after.heads);
}
fn draft_ref() -> RepositoryRef {
    RepositoryRef::draft("draft-1").unwrap()
}

#[test]
fn materialization_preserves_all_worktrees_and_roundtrips_exact_frozen_modes_bytes() {
    let fixture = fixture();
    let root = fixture.dir.path();
    let linked = tempfile::tempdir().unwrap();
    let path = linked.path().join("worktree");
    git(
        root,
        &["worktree", "add", "-qb", "topic", path.to_str().unwrap()],
    );
    fs::write(root.join("staged.py"), "staged\n").unwrap();
    git(root, &["add", "staged.py"]);
    fs::write(root.join("src/main.py"), "dirty worktree\n").unwrap();
    fs::write(root.join("untracked.txt"), "keep untracked\n").unwrap();
    fs::write(path.join("README.md"), "linked dirty\n").unwrap();
    let before = user_state(&fixture, vec![root.into(), path]);
    let request = request_id();
    let writer = SnapshotWriter::default();
    let materialized = save(&writer, &fixture, &fixture.frozen, None, &request).unwrap();
    assert_eq!(
        materialized.delivery().to_owned(),
        DraftDelivery::LocalSaved
    );
    assert_eq!(materialized.reference().to_owned(), draft_ref().name());
    let actual = read_draft(&fixture.catalog, "draft-1").unwrap().unwrap();
    assert_eq!(actual.revision, materialized.revision().to_owned());
    assert_eq!(actual.request_id().to_owned(), request);
    assert_eq!(actual.frozen, fixture.frozen);
    assert_eq!(
        save(
            &writer,
            &fixture,
            &fixture.frozen,
            None,
            actual.request_id()
        )
        .unwrap(),
        materialized
    );
    assert_user_state(&fixture, &before);
    assert!(!git(root, &["show", "main:docs/tower/arch/A.md"]).contains("Human edit"));
    assert!(!git(root, &["ls-tree", "-r", "--name-only", "main"]).contains(MANIFEST));
    assert_eq!(
        git(
            root,
            &["show", "-s", "--format=%an", materialized.revision()]
        ),
        "process:towermd-draft"
    );
}
#[test]
fn wrong_expected_request_content_and_forged_frozen_never_move_refs() {
    let fixture = fixture();
    let writer = SnapshotWriter::default();
    let request = request_id();
    let first = save(&writer, &fixture, &fixture.frozen, None, &request).unwrap();
    let mut edited = fixture.frozen.bundle.clone();
    edited
        .objects
        .get_mut("A")
        .unwrap()
        .text
        .push_str("next edit\n");
    let edited = freeze(&fixture.catalog, &edited).unwrap();
    assert!(matches!(
        save(&writer, &fixture, &edited, None, &request),
        Err(DraftSaveError::Conflict { .. })
    ));
    assert!(matches!(
        save(&writer, &fixture, &edited, None, &request_id()),
        Err(DraftSaveError::Conflict { .. })
    ));
    let mut forged = fixture.frozen.clone();
    forged
        .files
        .get_mut("docs/tower/arch/A.md")
        .unwrap()
        .text
        .push_str("unhashed");
    assert!(save(
        &writer,
        &fixture,
        &forged,
        Some(first.revision()),
        &request_id()
    )
    .is_err());
    let mut forged = fixture.frozen.clone();
    forged.files.insert(
        "README.md".into(),
        crate::baseline_draft::FrozenFile {
            mode: "100644".into(),
            sha256: "a".repeat(64),
            text: "take over".into(),
        },
    );
    assert!(save(
        &writer,
        &fixture,
        &forged,
        Some(first.revision()),
        &request_id()
    )
    .is_err());
    assert_eq!(
        repository_writer::direct_revision(fixture.dir.path(), &draft_ref()).unwrap(),
        Some(first.revision().to_owned())
    );
    assert!(read_draft(&fixture.catalog, "../escape").is_err());
    assert!(read_draft(&fixture.catalog, "publication-reserved").is_err());
}
#[test]
fn two_edits_push_fast_forward_and_remote_rejection_keeps_same_retry_commit() {
    let fixture = fixture();
    let root = fixture.dir.path();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-q"]);
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(root, &["push", "-q", "origin", "main"]);
    let writer = SnapshotWriter::default();
    let first = save(&writer, &fixture, &fixture.frozen, None, &request_id()).unwrap();
    assert_eq!(first.delivery().to_owned(), DraftDelivery::RemoteSaved);
    let mut edited = fixture.frozen.bundle.clone();
    edited
        .objects
        .get_mut("A")
        .unwrap()
        .text
        .push_str("second\n");
    let edited = freeze(&fixture.catalog, &edited).unwrap();
    let second = save(
        &writer,
        &fixture,
        &edited,
        Some(first.revision()),
        &request_id(),
    )
    .unwrap();
    assert_eq!(second.delivery().to_owned(), DraftDelivery::RemoteSaved);
    assert_eq!(
        git(
            root,
            &["rev-parse", &format!("{}^2", second.revision().to_owned())]
        ),
        first.revision().to_owned()
    );
    assert_eq!(
        git(remote.path(), &["rev-parse", draft_ref().name()]),
        second.revision().to_owned()
    );
    let mut third = edited.bundle.clone();
    third.objects.get_mut("A").unwrap().text.push_str("third\n");
    let third = freeze(&fixture.catalog, &third).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let hook = remote.path().join("hooks/pre-receive");
        fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let request = request_id();
    let pending = save(&writer, &fixture, &third, Some(second.revision()), &request).unwrap();
    #[cfg(unix)]
    {
        assert!(matches!(
            pending.delivery().to_owned(),
            DraftDelivery::LocalSavedRemotePending { .. }
        ));
        assert_eq!(
            git(remote.path(), &["rev-parse", draft_ref().name()]),
            second.revision().to_owned()
        );
        fs::remove_file(remote.path().join("hooks/pre-receive")).unwrap();
        let retried = save(&writer, &fixture, &third, Some(second.revision()), &request).unwrap();
        assert_eq!(retried.revision().to_owned(), pending.revision().to_owned());
        assert_eq!(retried.delivery().to_owned(), DraftDelivery::RemoteSaved);
    }
}
#[test]
fn same_writer_and_expired_or_replaced_main_lease_refuse_before_cas() {
    let fixture = fixture();
    let writer = SnapshotWriter::default();
    {
        let _lock = writer.writing.lock().unwrap();
        assert!(matches!(
            save(&writer, &fixture, &fixture.frozen, None, &request_id()),
            Err(DraftSaveError::Busy)
        ));
        assert_eq!(
            writer
                .write(
                    &fixture.session,
                    &fixture.catalog,
                    "host",
                    true,
                    || Ok(NOW),
                    &mut crate::test_support::outbox(&fixture.catalog)
                )
                .unwrap()
                .status,
            "deferred"
        );
    }
    let mut count = 0;
    let result = writer.save_draft(
        &fixture.session,
        &fixture.catalog,
        &fixture.frozen,
        &prepared(&fixture.frozen, None, &request_id()),
        || {
            count += 1;
            Ok(if count >= 3 {
                NOW + super::super::TTL + 1
            } else {
                NOW
            })
        },
    );
    assert!(result.is_err(), "{result:?}");
    assert!(
        repository_writer::direct_revision(fixture.dir.path(), &draft_ref())
            .unwrap()
            .is_none()
    );
}
#[test]
fn authority_and_real_competing_ref_are_rechecked_after_tree_preparation() {
    for action in ["authority", "ref", "symbolic"] {
        let fixture = fixture();
        let root = fixture.dir.path().canonicalize().unwrap();
        let revision = fixture.catalog.revision.clone();
        let hook_root = root.clone();
        crate::snapshot_git::tests::set_hook(
            root.clone(),
            Some(Arc::new(move |args| {
                if args != ["before-draft-cas-validation"] {
                    return;
                }
                if action == "authority" {
                    let tree = git(&hook_root, &["rev-parse", &format!("{revision}^{{tree}}")]);
                    let winner = git(
                        &hook_root,
                        &[
                            "commit-tree",
                            &tree,
                            "-p",
                            &revision,
                            "-m",
                            "authority advanced",
                        ],
                    );
                    git(
                        &hook_root,
                        &["update-ref", "refs/heads/main", &winner, &revision],
                    );
                } else if action == "ref" {
                    git(&hook_root, &["update-ref", draft_ref().name(), &revision]);
                } else {
                    git(
                        &hook_root,
                        &["symbolic-ref", draft_ref().name(), "refs/heads/main"],
                    );
                }
            })),
        );
        let result = save(
            &SnapshotWriter::default(),
            &fixture,
            &fixture.frozen,
            None,
            &request_id(),
        );
        crate::snapshot_git::tests::set_hook(root, None);
        assert!(result.is_err(), "{action}: {result:?}");
        if action == "authority" {
            assert!(
                repository_writer::direct_revision(fixture.dir.path(), &draft_ref())
                    .unwrap()
                    .is_none()
            );
        } else {
            assert_eq!(
                git(fixture.dir.path(), &["rev-parse", draft_ref().name()]),
                fixture.catalog.revision
            );
        }
    }
}
#[test]
fn reader_rejects_actual_bytes_extra_paths_wrong_parents_and_symbolic_refs() {
    for action in ["file", "extra", "parents", "symbolic"] {
        let fixture = fixture();
        let root = fixture.dir.path();
        let saved = save(
            &SnapshotWriter::default(),
            &fixture,
            &fixture.frozen,
            None,
            &request_id(),
        )
        .unwrap();
        if action == "symbolic" {
            git(
                root,
                &["symbolic-ref", draft_ref().name(), "refs/heads/main"],
            );
        } else {
            let session = Session::for_draft(root).unwrap();
            let mut files = vec![];
            if action == "file" {
                files.push(TreeFile {
                    path: "docs/tower/arch/A.md",
                    mode: "100644",
                    content: "forged bytes",
                });
            }
            if action == "extra" {
                files.push(TreeFile {
                    path: "README.md",
                    mode: "100644",
                    content: "unexpected unrelated file",
                });
            }
            let tree = session
                .build_tree(root, saved.revision(), &files, &[])
                .unwrap();
            let parent = if action == "parents" {
                saved.revision()
            } else {
                fixture.catalog.revision.as_str()
            };
            let altered = session
                .commit_tree(root, &tree, &[parent], "untrusted imported draft")
                .unwrap();
            session
                .cas_ref(root, &draft_ref(), &altered, saved.revision())
                .unwrap();
        }
        assert!(read_draft(&fixture.catalog, "draft-1").is_err(), "{action}");
    }
}

#[test]
fn real_remote_ref_race_is_pending_and_never_force_overwrites_the_winner() {
    let fixture = fixture();
    let root = fixture.dir.path().canonicalize().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-q"]);
    git(
        &root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(&root, &["push", "-q", "origin", "main"]);
    let hook_root = root.clone();
    let source = fixture.catalog.revision.clone();
    let winner = Arc::new(Mutex::new(None));
    let observed = winner.clone();
    crate::snapshot_git::tests::set_hook(
        root.clone(),
        Some(Arc::new(move |args| {
            if args.first() != Some(&"push") {
                return;
            }
            let tree = git(&hook_root, &["rev-parse", &format!("{source}^{{tree}}")]);
            let commit = git(
                &hook_root,
                &[
                    "commit-tree",
                    &tree,
                    "-p",
                    &source,
                    "-m",
                    "real remote competitor",
                ],
            );
            git(
                &hook_root,
                &[
                    "push",
                    "-q",
                    "origin",
                    &format!("{commit}:{}", draft_ref().name()),
                ],
            );
            *observed.lock().unwrap() = Some(commit);
        })),
    );
    let before = user_state(&fixture, vec![root.clone()]);
    let saved = save(
        &SnapshotWriter::default(),
        &fixture,
        &fixture.frozen,
        None,
        &request_id(),
    )
    .unwrap();
    crate::snapshot_git::tests::set_hook(root, None);
    assert!(matches!(
        saved.delivery().to_owned(),
        DraftDelivery::LocalSavedRemotePending { .. }
    ));
    assert_eq!(
        git(remote.path(), &["rev-parse", draft_ref().name()]),
        winner.lock().unwrap().as_ref().unwrap().as_str()
    );
    assert_eq!(
        repository_writer::direct_revision(fixture.dir.path(), &draft_ref())
            .unwrap()
            .as_deref(),
        Some(saved.revision())
    );
    assert_user_state(&fixture, &before);
}
#[test]
fn fresh_fence_replacement_after_preparation_never_materializes_a_ref() {
    let fixture = Arc::new(fixture());
    let root = fixture.dir.path().canonicalize().unwrap();
    let state = fixture.clone();
    crate::snapshot_git::tests::set_hook(
        root.clone(),
        Some(Arc::new(move |args| {
            if args != ["before-draft-cas-validation"] {
                return;
            }
            let mut live = state.session.lock().unwrap();
            let fence = live.leases(NOW).get("main").unwrap().fence();
            live.protocol()
                .unwrap()
                .lease_release(
                    crate::hub_protocol::ReleaseLease {
                        host: "host".into(),
                        fences: vec![fence],
                    },
                    NOW + 1,
                )
                .unwrap();
            live.protocol()
                .unwrap()
                .lease_acquire(
                    crate::hub_protocol::AcquireLease {
                        host: "other".into(),
                        names: vec!["main".into()],
                        holder: "other-writer".into(),
                        ttl_ms: 120_000,
                    },
                    NOW + 1,
                )
                .unwrap();
        })),
    );
    let saved = save(
        &SnapshotWriter::default(),
        &fixture,
        &fixture.frozen,
        None,
        &request_id(),
    );
    crate::snapshot_git::tests::set_hook(root, None);
    assert!(saved.is_err(), "{saved:?}");
    assert!(
        repository_writer::direct_revision(fixture.dir.path(), &draft_ref())
            .unwrap()
            .is_none()
    );
}
#[test]
fn reader_verifies_imported_clone_and_new_edits_cannot_change_fixed_source() {
    let fixture = fixture();
    let writer = SnapshotWriter::default();
    let saved = save(&writer, &fixture, &fixture.frozen, None, &request_id()).unwrap();
    let other = tempfile::tempdir().unwrap();
    let clone = other.path().join("clone");
    git(
        other.path(),
        &[
            "clone",
            "-q",
            "--no-hardlinks",
            fixture.dir.path().to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    git(
        &clone,
        &[
            "fetch",
            "-q",
            "origin",
            &format!("{}:{}", draft_ref().name(), draft_ref().name()),
        ],
    );
    let imported = Catalog::load(&clone).unwrap();
    assert!(!imported.common_dir.join("towermd").exists());
    let read = read_draft(&imported, "draft-1").unwrap().unwrap();
    assert_eq!(read.revision(), saved.revision().to_owned());
    assert_eq!(read.frozen(), &fixture.frozen);
    assert!(
        !imported.common_dir.join("towermd").exists(),
        "reader created repository runtime state"
    );
    fs::write(
        fixture.dir.path().join("README.md"),
        "authority ordinary advance\n",
    )
    .unwrap();
    git(fixture.dir.path(), &["add", "README.md"]);
    git(fixture.dir.path(), &["commit", "-qm", "main advanced"]);
    let current = Catalog::load(fixture.dir.path()).unwrap();
    assert_eq!(
        read_draft(&current, "draft-1").unwrap().unwrap().revision(),
        saved.revision().to_owned()
    );
    let mut bundle = fixture.frozen.bundle.clone();
    bundle.source_revision = current.revision.clone();
    let changed = freeze(&current, &bundle).unwrap();
    let result = writer.save_draft(
        &fixture.session,
        &current,
        &changed,
        &prepared(&changed, Some(saved.revision()), &request_id()),
        || Ok(NOW),
    );
    assert!(
        matches!(result, Err(DraftSaveError::Conflict { .. })),
        "{result:?}"
    );
}

#[test]
fn manifest_roundtrips_complete_typed_request_and_same_uuid_cannot_change_actor_host_run_or_body() {
    let fixture = fixture();
    let writer = SnapshotWriter::default();
    let id = request_id();
    let original = prepared(&fixture.frozen, None, &id);
    let first = writer
        .save_draft(
            &fixture.session,
            &fixture.catalog,
            &fixture.frozen,
            &original,
            || Ok(NOW),
        )
        .unwrap();
    let saved = read_draft(&fixture.catalog, "draft-1").unwrap().unwrap();
    assert_eq!(saved.request(), &original);
    for coordinate in ["actor", "host", "run", "submit"] {
        let mut body = original.body().clone();
        match coordinate {
            "actor" => body.actor = "human:another".into(),
            "host" => body.host = "another-host".into(),
            "run" => body.run = Some("another-run".into()),
            _ => {
                if let DraftSaveAction::SubmitBaseline { submit } = &mut body.action {
                    submit.risks.push("different original input".into());
                }
            }
        }
        let changed = PreparedDraftSaveRequest::prepare(body, &id).unwrap();
        let result = writer.save_draft(
            &fixture.session,
            &fixture.catalog,
            &fixture.frozen,
            &changed,
            || Ok(NOW),
        );
        assert!(
            matches!(result, Err(DraftSaveError::Conflict { .. })),
            "{coordinate}: {result:?}"
        );
    }
    let recovered = SnapshotWriter::default()
        .save_draft(
            &fixture.session,
            &fixture.catalog,
            &fixture.frozen,
            saved.request(),
            || Ok(NOW),
        )
        .unwrap();
    assert_eq!(recovered, first);
    let root = fixture.dir.path();
    let git = Session::for_draft(root).unwrap();
    let text = git
        .run(
            root,
            &["show", &format!("{}:{MANIFEST}", first.revision())],
            None,
        )
        .unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
    manifest["request_body"]["actor"] = json!("human:forged");
    let forged = format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap());
    let tree = git
        .build_tree(
            root,
            first.revision(),
            &[TreeFile {
                path: MANIFEST,
                mode: "100644",
                content: &forged,
            }],
            &[],
        )
        .unwrap();
    let altered = git
        .commit_tree(
            root,
            &tree,
            &[fixture.catalog.revision.as_str()],
            "forged request body",
        )
        .unwrap();
    git.cas_ref(root, &draft_ref(), &altered, first.revision())
        .unwrap();
    assert!(read_draft(&fixture.catalog, "draft-1").is_err());
}
#[test]
fn historical_reader_survives_published_or_changed_slow_layer_but_new_save_fails_closed() {
    for action in ["tag", "slow"] {
        let fixture = fixture();
        let writer = SnapshotWriter::default();
        let first = save(&writer, &fixture, &fixture.frozen, None, &request_id()).unwrap();
        if action == "tag" {
            git(
                fixture.dir.path(),
                &["tag", "towermd/B0", &fixture.catalog.revision],
            );
        } else {
            fs::write(
                fixture.dir.path().join("docs/tower/ordinary.md"),
                "changed protected directory\n",
            )
            .unwrap();
            git(fixture.dir.path(), &["add", "docs/tower/ordinary.md"]);
            git(
                fixture.dir.path(),
                &["commit", "-qm", "slow directory advance"],
            );
        }
        let current = Catalog::load(fixture.dir.path()).unwrap();
        let historical = read_draft(&current, "draft-1").unwrap().unwrap();
        assert_eq!(historical.revision(), first.revision());
        assert_eq!(historical.frozen(), &fixture.frozen);
        let request = prepared(&fixture.frozen, Some(first.revision()), &request_id());
        let result = writer.save_draft(
            &fixture.session,
            &current,
            &fixture.frozen,
            &request,
            || Ok(NOW),
        );
        assert!(result.is_err(), "{action}: {result:?}");
        assert_eq!(
            repository_writer::direct_revision(fixture.dir.path(), &draft_ref())
                .unwrap()
                .as_deref(),
            Some(first.revision())
        );
    }
}

#[test]
fn actual_draft_git_boundary_keeps_session_available_and_snapshot_busy_on_same_writer() {
    let fixture = Arc::new(fixture());
    let writer = Arc::new(SnapshotWriter::default());
    let state = fixture.clone();
    let snapshot_writer = writer.clone();
    let seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = seen.clone();
    let root = fixture.dir.path().canonicalize().unwrap();
    crate::snapshot_git::tests::set_hook(
        root.clone(),
        Some(Arc::new(move |args| {
            if args != ["before-draft-cas-validation"] {
                return;
            }
            assert!(
                state.session.try_lock().is_ok(),
                "Git boundary retained session lock"
            );
            let attempt = snapshot_writer.write_observed(
                &state.session,
                &state.catalog,
                "host",
                true,
                || Ok(NOW),
                &mut crate::test_support::outbox(&state.catalog),
            );
            assert!(attempt.inflight().is_none());
            assert_eq!(attempt.into_result().unwrap().status, "deferred");
            observed.store(true, std::sync::atomic::Ordering::SeqCst);
        })),
    );
    let saved = save(&writer, &fixture, &fixture.frozen, None, &request_id());
    crate::snapshot_git::tests::set_hook(root, None);
    assert!(saved.is_ok(), "{saved:?}");
    assert!(seen.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn current_user_draft_reader_excludes_candidates_and_rejects_symbolic_aliases() {
    let f = fixture();
    let writer = SnapshotWriter::default();
    let actual = save(&writer, &f, &f.frozen, None, &request_id()).unwrap();
    let refs = draft_ref_fingerprint(&f.catalog).unwrap();
    git(
        f.dir.path(),
        &[
            "update-ref",
            "refs/towermd/drafts/publication-reader-fixture",
            &f.catalog.revision,
        ],
    );
    assert_eq!(draft_ref_fingerprint(&f.catalog).unwrap(), refs);
    let requests = read_draft_requests(&f.catalog).unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        &requests[0],
        read_draft(&f.catalog, "draft-1")
            .unwrap()
            .unwrap()
            .request()
    );
    assert_eq!(requests[0].draft_id(), actual.result().draft_id);
    git(
        f.dir.path(),
        &[
            "symbolic-ref",
            "refs/towermd/drafts/alias",
            "refs/towermd/drafts/draft-1",
        ],
    );
    assert!(read_draft_requests(&f.catalog)
        .unwrap_err()
        .to_string()
        .contains("direct"));
}

#[test]
fn current_user_draft_reader_rechecks_the_whole_ref_set_after_reading() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let f = fixture();
    let writer = SnapshotWriter::default();
    save(&writer, &f, &f.frozen, None, &request_id()).unwrap();
    let root = f.dir.path().canonicalize().unwrap();
    let observed = root.clone();
    let revision = f.catalog.revision.clone();
    let visits = Arc::new(AtomicUsize::new(0));
    let counted = visits.clone();
    crate::snapshot_git::tests::set_hook(
        root.clone(),
        Some(Arc::new(move |args| {
            if args
                == [
                    "for-each-ref",
                    "--format=%(refname)%09%(objectname)%09%(symref)",
                    "refs/towermd/drafts/",
                ]
                && counted.fetch_add(1, Ordering::SeqCst) == 1
            {
                git(
                    &observed,
                    &[
                        "update-ref",
                        "refs/towermd/drafts/new-during-read",
                        &revision,
                    ],
                );
            }
        })),
    );
    let result = read_draft_requests(&f.catalog);
    crate::snapshot_git::tests::set_hook(root, None);
    assert!(
        matches!(&result,Err(DraftSaveError::Conflict{reason}) if reason.contains("ref set changed")),
        "{result:?}"
    );
    assert_eq!(visits.load(Ordering::SeqCst), 2);
}

// Alter actual Git payload bytes while retaining a complete canonical manifest.
fn replace_draft_file(f: &Fixture, revision: &str, path: &str, mode: &str, text: &str) -> String {
    let writer = Session::for_draft(f.dir.path()).unwrap();
    let tree = writer
        .build_tree(
            f.dir.path(),
            revision,
            &[TreeFile {
                path,
                mode,
                content: text,
            }],
            &[],
        )
        .unwrap();
    let changed = writer
        .commit_tree(
            f.dir.path(),
            &tree,
            &[&f.catalog.revision],
            "actual negative draft tree",
        )
        .unwrap();
    git(f.dir.path(), &["update-ref", draft_ref().name(), &changed]);
    changed
}

#[test]
fn canonical_request_scan_reserves_uuid_without_certifying_a_corrupt_saved_payload() {
    let f = fixture();
    let writer = SnapshotWriter::default();
    let id = request_id();
    let original = prepared(&f.frozen, None, &id);
    let saved = writer
        .save_draft(&f.session, &f.catalog, &f.frozen, &original, || Ok(NOW))
        .unwrap();
    let file = &f.frozen.files["docs/tower/arch/A.md"];
    let changed = replace_draft_file(
        &f,
        saved.revision(),
        "docs/tower/arch/A.md",
        &file.mode,
        "corrupted actual saved bytes",
    );
    assert_eq!(
        read_draft_requests(&f.catalog).unwrap(),
        vec![original.clone()]
    );
    assert!(read_draft(&f.catalog, "draft-1")
        .unwrap_err()
        .to_string()
        .contains("actual file modes/bytes"));
    let mut outbox = crate::test_support::outbox(&f.catalog);
    let inputs = outbox.read_inputs().unwrap();
    let status = crate::request_ledger::read_baseline_request(&f.catalog, &inputs, &id)
        .unwrap()
        .unwrap();
    let crate::request_ledger::BaselineRequestStatus::DraftSave {
        request_body,
        p3_request,
        receipt,
    } = status
    else {
        panic!("request reservation is not an approval");
    };
    assert_eq!(&request_body, original.body());
    assert_eq!(&p3_request, original.metadata());
    assert!(receipt.is_none());
    let journal = f.catalog.common_dir.join("towermd/journal.jsonl");
    let before = fs::read(&journal).unwrap();
    assert!(ProjectSession::retry_draft_save_shared(
        &f.session,
        &f.catalog,
        &mut outbox,
        &original,
        NOW + 1
    )
    .unwrap()
    .is_none());
    let mut other = original.body().clone();
    other.host = "different-host".into();
    let other = PreparedDraftSaveRequest::prepare(other, &id).unwrap();
    let error = ProjectSession::retry_draft_save_shared(
        &f.session,
        &f.catalog,
        &mut outbox,
        &other,
        NOW + 2,
    )
    .unwrap_err();
    assert!(error.contains("request_id reused"), "{error}");
    assert!(writer
        .save_draft(&f.session, &f.catalog, &f.frozen, &original, || Ok(NOW + 3))
        .is_err());
    let error = crate::baseline_review::review_baseline_shared(
        &f.session,
        &f.catalog,
        &mut outbox,
        &crate::baseline_review::ReviewAction {
            request_id: request_id(),
            binding: crate::baseline_review::review_binding(&f.frozen, &changed),
            verdict: crate::baseline_review::Verdict::Approved,
            comment: "negative protocol test, no GUI acceptance".into(),
        },
        "human:test",
        "host",
        NOW + 4,
    )
    .unwrap_err();
    assert!(error.contains("actual file modes/bytes"), "{error}");
    assert!(outbox.read_inputs().unwrap().confirmed().is_empty());
    assert_eq!(fs::read(journal).unwrap(), before);
}

#[test]
fn request_identity_scan_rejects_foreign_project_schema_hash_and_noncanonical_manifest() {
    let f = fixture();
    let writer = SnapshotWriter::default();
    let saved = save(&writer, &f, &f.frozen, None, &request_id()).unwrap();
    let reader = Session::for_network(f.dir.path()).unwrap();
    let text = reader
        .run(
            f.dir.path(),
            &["show", &format!("{}:{MANIFEST}", saved.revision())],
            None,
        )
        .unwrap();
    for mutation in ["project", "schema", "hash", "canonical"] {
        let mut manifest: Manifest = serde_json::from_str(&text).unwrap();
        match mutation {
            "project" => {
                manifest.request_body.project_id = "another-project".into();
                manifest.frozen.bundle.project_id = "another-project".into();
                let DraftSaveAction::SubmitBaseline { submit } = &mut manifest.request_body.action
                else {
                    unreachable!()
                };
                submit.project_id = "another-project".into();
                manifest.p3_request.body_hash =
                    recovery_contract::canonical_hash(&manifest.request_body).unwrap();
            }
            "schema" => manifest.schema = 2,
            "hash" => manifest.p3_request.body_hash = "0".repeat(64),
            _ => {}
        }
        let mut altered = manifest_bytes(&manifest).unwrap();
        if mutation == "canonical" {
            altered.push(' ');
        }
        replace_draft_file(&f, saved.revision(), MANIFEST, "100644", &altered);
        assert!(read_draft_requests(&f.catalog).is_err(), "{mutation}");
        assert!(read_draft(&f.catalog, "draft-1").is_err(), "{mutation}");
    }
    let tree = git(
        f.dir.path(),
        &["rev-parse", &format!("{}^{{tree}}", saved.revision())],
    );
    git(f.dir.path(), &["update-ref", draft_ref().name(), &tree]);
    assert!(read_draft_requests(&f.catalog)
        .unwrap_err()
        .to_string()
        .contains("not a commit"));
}
