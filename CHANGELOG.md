# Changelog

## Unreleased

### Fixed

- Keep documents with invalid explicit layers readable and protected, preserving their text and reporting path diagnostics throughout Canvas views. Publication and deletion still require valid slow-layer objects.
- Limit run-binding hints to active runs on the current host while retaining complete run history in status.
- Preserve Canvas projection with fractional and large finite values in unknown extensions. Invalid geometry affects only its node, and imported Canvas bytes remain unchanged.
- Report unresolved interface bindings when the interface catalog has not been loaded, preserving other Canvas diagnostics.

### Changed

- Promote private snapshots onto writable main without new business events, retaining newer code and all archive months. Retry remote non-fast-forward races up to three total attempts while keeping status and renewals responsive; changed policy or project context requires replanning. Loading another project's archive keeps active project reads and renewals responsive.

- Persist snapshots to a private, non-branch Git ref when local `main` is checked out, preserving every user worktree and index. Read and recover from the newest validated main/private checkpoint, reject conflicting boundaries, and retain every archive month.

- Migrate local hubs to a strictly validated append-only journal and compact acknowledged snapshot prefixes; idle commands reuse incremental projections. Older subscriptions reload the checkpoint, and uncertain writes require reopening the hub.
- Record lease grants/releases separately from L5 task claims/releases, preserve all run fences, and expose structured lease owners on contention.

- Prevent concurrent first commands from hanging when the daemon inherits a client output pipe, while preserving startup error reporting.

- Read and renew active runs offline from the most recently cached authoritative main; background fetch failures are reported without blocking commands.
- Initialization no longer contacts the daemon or renews runs. New non-interactive projects require an explicit `--docs-dir`.
- Remote push preflight during initialization is best effort, negotiates the fetched remote main, and reports manual skill installation guidance without changing harness configuration.

### Added

- Verify project sessions against complete recovery history, preserve canonical Task/Run outcomes and original replay identities, and install acknowledged recovery checkpoints atomically. App recovery confirmation remains pending.

- Start or discover the shared daemon with `daemon ensure`, without renewing task leases or entering project command and message-delivery routes.

- Prepare matching project versions and bilingual release notes with offline checks, specific cache-miss diagnostics and recovery of owned writes after failures, preserving additional edits.

- Audit release inputs across all worktrees, local branches and stashes, retaining other work and rejecting missing decisions or changed inputs.

- Add reusable release checks for bilingual changelogs, matching versions, ad-hoc-only packaging, exact asset sets and download checksums. Packaging and publication remain pending.

- Select Git projects and guide explicit offline initialization while preserving existing commits, staged changes and documents. Reuse existing configuration, retain CLI diagnostics, prevent initialization while project selection is pending, and isolate Git overrides that could redirect the chosen worktree.

- Prepare architecture-specific CLI sidecars for desktop builds using Cargo's actual output directory, and add fixed native CLI invocation with original diagnostics. Daemon integration remains pending.

- Add a macOS development desktop foundation with project-local pinned tooling.

- Add core recovery foundations: shared verification/review/publication data and stable hashes, project-bound durable pending/confirmed input validation, and Git tree/base-chain guards. Read actual Git snapshots, complete archives and original receipts without creating local state, and build deterministic Task merge/publication proofs and canonical events. Validate G5 in cold clones without retaining old commits, and distinguish snapshot, Task and publication footers. Contract fixtures cover these consumers; durable producers and App wiring remain deferred, with no new completion or approval command.

- Preview deterministic candidate modules and Canvas layouts with `baseline scan`, including source coverage, bounded Git co-change analysis, and unexecuted test plans. No baseline, task, or evidence is published; S5 execution remains deferred.
- Read Canvas and object views from authoritative commits while preserving original Canvas bytes and unknown fields; runtime state never changes stored layout.

- Persist manual and periodic snapshots to authoritative `main`, retaining non-heartbeat deduplication receipts in the hub checkpoint while omitting renewal heartbeats from state and monthly archives.
- Request disaster recovery without bypassing App approval. Recovery foundations persist new sequence/epoch boundaries before installing local state; CLI approval and execution remain unavailable in M0.
- Check origin push negotiation during initialization without pushing changes, and explain the limits of dry-run checks.

- Coordinate local tasks through a single per-user daemon: atomic module leases, worktree-aware runs, durable event replay, and `status` / `task add|claim|show|release` commands.
- Read project policy from authoritative `main`, enforce task path limits, and reject independent local clones sharing a project ID.

- Initialize an existing Git worktree with `towermd init`: create a stable UUIDv7 project ID, project configuration, Agent guidance, and a chosen documentation directory without committing or overwriting existing documents.
- Read and edit OKF documents while retaining unknown fields; unchanged documents round-trip byte for byte.
