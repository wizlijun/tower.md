# Changelog

## Unreleased

- Re-run verification after reviewing a draft, retaining the actual commands, logs and tree-bound results. Explicitly approve the frozen contents in the App to publish a baseline and its Git tag.
- Retry uncertain publication using the original approval, checking the real remote tag and main history. Keep the user's checkout and index unchanged.
- Persist project events before reporting success, write replay-checked snapshots, and restore a lost Hub only after explicit App confirmation. Retrying the same confirmation does not restart recovery.
- Add real task creation, worktree-bound claims, module leases, status and release commands. Task admission requires an actually published, human-approved baseline; self-reported completion does not count as progress.

- Open an interactive component sample without a repository. Explore a minimal baseline, task, question, verification and review flow on one canvas; sample data is clearly labeled and never writes to Git or starts commands.

- Review unapproved drafts directly on the canvas: rename, move, merge and split modules, add or remove interfaces, confirm verification commands, and reference committed Markdown files.
- Preserve each human edit as an immutable draft revision. Mark changed structure or commands as needing verification, retain original evidence, and retry uncertain saves without duplicating edits or changing main.

- Submit an Agent's cold-start synthesis using the shared JSON Schema, retaining module boundaries and real verification evidence. View intentions, interfaces, acceptance criteria, risks and questions on the same canvas.
- Keep synthesis originals in immutable private Git draft refs, with idempotent retries and private-ref publication when origin exists. Recover drafts after cache deletion; clearly flag outdated sources and missing verification logs without approving a baseline.

- Run cold-start verification against the selected commit in a detached worktree. Show real pass/fail/no-test results, command exit codes, bounded logs and tree-bound reports on the canvas without moving nodes; verification does not approve a baseline.
- Cancel verification and clean up process groups and temporary worktrees. A separate worker also detects daemon exit; interrupted scans remain interrupted after restart, and old daemon versions are explicitly rejected.

- Generate read-only cold-start candidates from a fixed authority commit, using source directories and bounded co-change history. Inspect real Module drafts, coverage, suggested commands and missing-test suggestions; candidates remain unapproved.
- Share project reads and asynchronous scans between the desktop App and CLI through the bundled local daemon. Reuse linked worktrees, recover cached reports after daemon restart, and rebuild deleted scan caches without changing worktrees, indexes or main.

- Reset the implementation to the spec-aligned basic framework, then rebuild the capabilities listed above. Older version sections remain historical records and do not describe the current implementation's scope.
- Add a minimal read-only desktop canvas for committed JSON Canvas files and Module documents. Open a Git repository with the native folder picker, inspect node source text, and use the canvas context menu to refresh, fit the view or read project details.
- Read one authority commit consistently: local main without origin, or the fetched origin/main when origin exists. Preserve committed coordinates, report missing or invalid references, and keep uncommitted changes outside the view.
- Keep the main window focused on the canvas. Cancelling project selection preserves the current project; project details and unplaced modules appear only on request. Full M1 acceptance remains in progress.

## v0.1.0 — 2026-10-20

- Open proposals directly from the live canvas, review complete changes, and approve later baselines. Recheck affected tasks against the exact baseline, run, and leases.
- Show read-only diagnostics for protected changes and failed gates, retaining original evidence until a new actual check updates their state.

- Verify bundled command-line code across macOS ad-hoc re-signing and record the actual packaged binary hash.
- Validate captured proposal approvals, task rechecks, and instruction delivery against their original events before accepting release evidence.
- Send scoped instructions from the live workspace, pause new task claims, and explicitly confirm the current resume scope. Keep project pauses across hub rebuilds.
- Deliver stable messages at CLI checkpoints and acknowledge only complete output; keep parallel verification responsive by scheduling maintenance at actual renewal boundaries.
- Show changes since the last normal canvas close and actual unresolved dependency chains without moving nodes.

- Verify captured task, run, evidence, human-action and package identities before publishing; reject incomplete records and changes to accepted runtime sources.

- Keep the native skill package version in sync with the App during release preparation; preserve concurrent edits if preparation fails.

- Submit source-bound private change proposals with `towermd propose`; retry the original request safely and preserve historical receipts.

- Put the live map before maintenance forms, and distinguish a directory preview’s HEAD from authoritative main so tasks remain reachable from other branches.

- Automatically read real project state into a shared live canvas with module-to-task drill-down, run/evidence timelines, and up to three attention items; open the original question or review directly from the map.
- Provide native workflow skill sources and manual installation instructions for Claude Code and Codex. Support source-bound proposals and App review of later baselines.

- Keep owned verification runs renewed within a fixed one-hour deadline; confirm real process cleanup before expiry or re-execution, and preserve original receipts across recovery.

- Read real task, run, and verification records in the App with an explicit refresh; show the verified sequence and capture time, preserve history after daemon restarts, and distinguish verification success from certified merges. This read does not renew tasks or acknowledge messages.

### Fixed

- Release verifying and held runs during hub recovery so expired run bindings cannot prevent a fresh claim.

- Reduce repeated Git reads while verifying baseline publications, retaining source, protected-file and approval checks.

- Release project bindings explicitly when the daemon drops them, including inherited file handles, so reopening the same clone does not falsely report it as occupied.

- Keep live task events connected while durable claims and releases update the recovery outbox. Actual proof failures still close the subscription explicitly.
- Reduce idle background Git probes while retaining actual source checks for business operations and snapshot writes.
- Persist task claims and releases in the recovery outbox before reporting success. Uncertain retries retain the original event and run identity, while changed recovery sources leave the last verified state intact.
- Allow a new task claim after an expired original request has been durably reconciled, without restarting the daemon. Failed source or proof checks continue to preserve the original request.
- Keep lease renewals responsive during snapshot preparation by moving pending task reconciliation to background work and reusing the prepared write context.
- Recognize the desktop App's local shared core package during release preparation, while retaining exact package/version validation and rejecting unknown packages or external replacements.
- Cancel creation when a draft-only interface or temporary split module is removed, avoiding fictitious source deletions that blocked B0 publication. Real source deletions and original historical drafts/receipts remain intact.
- Keep the original draft or review request pending when reading its status fails or recovery sources change during proof. Explicit retries retain the same UUID and body rather than resolving the queue as rejected.
- Persist actual Interface titles on bound canvas relations, using the interface ID when no title exists. Editing or renaming an interface updates its label while preserving ordinary relation labels.
- Distinguish absent verification fields from explicit null or legacy text in saved-object projections, preserving original text and preventing accidental replacement with an empty list.
- Preserve `CI` during unattended unsigned App builds so pnpm can run without a TTY.
- Correct CLI help for private snapshots and recovery requests awaiting App confirmation.
- Preserve snapshot deadlines and newer completion or failure state when older status queries, writer callbacks, or recovery callbacks return late.
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

- Add a source-bound proposal core with durable original receipts and recovery; connect proposal submission and later-baseline review through the public CLI and App.

- Generate task commit trailers from durable verification and review evidence, certify actual main entries through the shared proof, and release only the completed run's original module leases.

- Preserve the exact analyzed payload in proposal drafts and reject payload edits that invalidate its recorded impact.

- Verify committed tasks asynchronously with `towermd finish`, wait by evidence ID, and run independent modules concurrently. Preserve original requests and actual reports across confirmation failures and restarts; an orphaned running job is interrupted rather than rerun.

- Answer current task questions explicitly in the App, preserving original-request retries and read-only answers after recovery. The CLI shows the question ID before waiting.

- Ask and relay answers from the task CLI, with durable request retries, bounded 90-second waits, a ten-minute live-wait budget and unattended attention after thirty minutes. Historical requests never restore expired run authority.

- Review an immutable task plan (G1) or committed tree (G4) in the App, with explicit verdicts and durable original-request retries. Historical approvals remain read-only after recovery.
- Check the current task run with `towermd check`, reporting committed inputs and live G0/G2/G5 gates without executing verification or creating completion evidence.
- Read and explicitly confirm an original CLI-issued hub recovery request in the App, retaining its source and expiry. Native admission rejects changed project/instance context, requires viewing before confirmation, and preserves uncertain results without automatic retry.
- Compare the actual main canvas with the exact reviewed draft using solid and dashed layers, switch views without saving, and highlight both resolved Interface modules with explicit missing-node diagnostics. The comparison preserves coordinates and remains read-only.
- Explicitly create or recover a saved draft from the verified current baseline and fixed main commit, then edit and reopen it on the same canvas. Keep original pending creation requests visible across restarts; reread the queue after uncertain preparation, and reopen the same saved draft ID after a confirmed creation instead of creating again. Review later drafts explicitly in the App; publishing a new baseline marks affected tasks for recheck.
- After a scan completes, show its ID, source commit and commands for the existing Agent to prepare and submit S6 JSON. Explicitly open the submitted draft for that same scan without copying its ID; the App does not synthesize, submit or approve S6 automatically.
- Add five S7 structural editing actions: merge/split modules, add/edit/remove interfaces, and confirm verification commands. Configure working directories, programs and individual arguments explicitly; legacy display strings require deliberate manual replacement and are never parsed or executed during saving.
- Provide a macOS Apple Silicon 0.0.0 current-baseline drafting development preview with the release profile and a bundled CLI, retaining the S6 handoff and S7 forms. Preserve the earlier `s7-handoff`, `s7-editing` and `draft-review` bundles and their original manifests separately. Static package checks and automated workflows pass; GUI acceptance and a formal release remain pending.
- Submit S6 JSON using the shared schema, edit saved drafts in the App as cards and arbitrary relations, and review exact content with real verification evidence. Explicit approval can publish B0 through the single leased writer; checked-out main and remote failures remain pending. Original requests and decisions survive retries and recovery. Milestone and desktop acceptance are recorded separately for each package.

- Run coldstart scans and selected-commit verification in the background. Wait by scan ID, read candidate drafts with per-module evidence, and recover confirmed reports after restart without rerunning tests. Interrupted verification remains explicit; scans preserve user files and indexes and never approve a baseline or complete a task.

- View real Git projects in the App: read the authoritative canvas and document text, or preview committed directories without initialization. Refresh and switch projects, with explicit empty and error states; display reads do not start the daemon, renew runs, or modify the repository.

- Start with real project selection; show demo data only when explicitly requested. Initialized projects can generate actual candidate canvases and inspect module text, passed/failed/no-test results and the original shared verification report. Candidates remain unapproved and read-only.

- Schedule verified merge, resolved hold, submitted proposal and published baseline events in a fixed 25-second snapshot window, report the last snapshot and deferral state, and retain one failure notification per episode across restarts.
- Include actual module paths in coldstart test suggestions and aggregate shared verification reports into separate module results.

- Check frozen release materials and source bindings; refuse production publication until real acceptance records are connected.

- Add a reproducible coldstart draft core that preserves fixed-commit inputs and Unicode module IDs, with matching Module and Canvas data. Private Git draft saving and limited App editing are now connected; full milestone acceptance is recorded separately for each package.

- Plan coldstart modules with readable stable IDs and balanced directory groups; prefer module-owned test commands, retain unconfirmed candidates and deduplicate shared commands.

- Add a shared selected-commit verification core with detached worktrees, inherited tool environments, sealed local logs and bounded process cleanup. Use the same verifier for coldstart and task finish.
- Restore lost local hubs through saved App-only confirmation requests, with read-only recovery status, unchanged task leases and safe retry diagnostics after a committed boundary cannot be installed. Confirm recovery explicitly in the App; human acceptance is recorded separately for each version.
- Verify App/DMG contents, archive native skills from frozen Git blobs, and assemble nine release assets with matching manifests and checksums; reject dependency escapes, build receipt drift and changed artifacts.
- Build locked App/DMG bundles from clean frozen tags, isolating architecture outputs and rejecting incorrect CLI architecture, unsafe configuration, source or lock drift, and stale bundles.
- Add an explicit-target unsigned macOS App build entry that stages the matching CLI, checks configuration and required files, and stops on failed build steps.
- Verify project sessions against complete recovery history, preserve canonical Task/Run outcomes and original replay identities, and install acknowledged recovery checkpoints atomically.

- Start or discover the shared daemon with `daemon ensure`, without renewing task leases or entering project command and message-delivery routes.

- Prepare matching project versions and bilingual release notes with offline checks, specific cache-miss diagnostics and recovery of owned writes after failures, preserving additional edits.

- Audit release inputs across all worktrees, local branches and stashes, retaining other work and rejecting missing decisions or changed inputs.

- Add reusable release checks for bilingual changelogs, matching versions, ad-hoc-only packaging, exact asset sets and download checksums. Tool checks alone do not certify a package or publication.

- Select Git projects and guide explicit offline initialization while preserving existing commits, staged changes and documents. Reuse existing configuration, retain CLI diagnostics, prevent initialization while project selection is pending, and isolate Git overrides that could redirect the chosen worktree.

- Prepare architecture-specific CLI sidecars for desktop builds using Cargo's actual output directory, retain original native CLI diagnostics, and connect the App to the checked local daemon instance.

- Add a macOS development desktop foundation with project-local pinned tooling.

- Add core recovery foundations: shared verification/review/publication data and stable hashes, project-bound durable pending/confirmed input validation, and Git tree/base-chain guards. Read actual Git snapshots, complete archives and original receipts without creating local state, and build deterministic Task merge/publication proofs and canonical events. Validate G5 in cold clones without retaining old commits, and distinguish snapshot, Task and publication footers. Coldstart evidence and baseline draft/review/publication producers are connected; task finish and the App use these proofs, with no CLI approval command.

- Plan deterministic candidate modules and Canvas layouts with `baseline scan`, including source coverage, bounded Git co-change analysis, and test plans. Scans execute selected-commit verification and retain evidence without activating a baseline or completing a task.
- Read Canvas and object views from authoritative commits while preserving original Canvas bytes and unknown fields; runtime state never changes stored layout.

- Persist manual and periodic snapshots to authoritative `main`, retaining non-heartbeat deduplication receipts in the hub checkpoint while omitting renewal heartbeats from state and monthly archives.
- Request disaster recovery without bypassing App approval. Recovery foundations persist new sequence/epoch boundaries before installing local state; CLI approval and execution remain unavailable in M0.
- Check origin push negotiation during initialization without pushing changes, and explain the limits of dry-run checks.

- Coordinate local tasks through a single per-user daemon: atomic module leases, worktree-aware runs, durable event replay, and `status` / `task add|claim|show|release` commands.
- Read project policy from authoritative `main`, enforce task path limits, and reject independent local clones sharing a project ID.

- Initialize an existing Git worktree with `towermd init`: create a stable UUIDv7 project ID, project configuration, Agent guidance, and a chosen documentation directory without committing or overwriting existing documents.
- Read and edit OKF documents while retaining unknown fields; unchanged documents round-trip byte for byte.
