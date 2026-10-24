import { Channel, invoke } from '@tauri-apps/api/core'
import type { SetBy } from './naming'

export interface ProjectSelection {
  token: string
  path: string
}

export interface CanvasNode {
  id: string
  kind: 'text' | 'file' | 'link' | 'group'
  title: string
  content: string
  x: number
  y: number
  width: number
  height: number
  file?: string
  url?: string
  object_id?: string
  object_type?: string
  /** FR-188: who set the object, derived by the daemon from its internal layer. */
  set_by?: SetBy
  region?: string
}

export interface CanvasEdge {
  id: string
  from_node: string
  to_node: string
  label?: string
  from_end?: string
  to_end?: string
  from_side?: string
  to_side?: string
  color?: string
}

export interface ProjectView {
  title: string
  reference: string
  revision: string
  docs_dir: string | null
  diagnostics: string[]
  nodes: CanvasNode[]
  edges: CanvasEdge[]
  unplaced: CanvasNode[]
}

export interface ScanModule {
  id: string
  title: string
  paths: string[]
  files: string[]
  confidence: number
  rationale: string
  command_ids: string[]
  suggested_task?: string | null
}

export interface ScanCommand {
  id: string
  directory: string
  command: string
  modules: string[]
}

export type VerificationStatus = 'passed' | 'failed' | 'no_tests' | 'timed_out' | 'cancelled' | 'not_run'

export interface CommandResult {
  id: string
  directory: string
  argv: string[]
  modules: string[]
  status: VerificationStatus
  exit_code: number | null
  duration_ms: number
  log: string | null
  log_sha256: string | null
  log_excerpt: string
  log_truncated: boolean
  error: string | null
}

export interface ModuleResult {
  module: string
  status: VerificationStatus
  command_ids: string[]
  shared: boolean
}

export interface VerificationReport {
  actor: string
  revision: string
  tree: string
  started_at: string
  finished_at: string
  commands: CommandResult[]
  modules: ModuleResult[]
  cleanup_complete: boolean
}

export interface ScanReport {
  id: string
  project_id: string
  reference: string
  revision: string
  tree: string
  view: ProjectView
  modules: ScanModule[]
  commands: ScanCommand[]
  unassigned: string[]
  source_file_count: number
  assigned_file_count: number
  coverage: number
  diagnostics: string[]
  hotspots: { path: string; commits: number }[]
  couplings: { from: string; to: string; commits: number }[]
  /** Module-level import dependencies (S2); absent in older scans. */
  dependencies?: { from: string; to: string; imports: number }[]
  /** The 8.2 coverage recheck; absent in older scans. */
  checks?: CoverageChecks | null
  verification?: VerificationReport | null
  /** J4: the approved baseline this scan re-enters as a draft; absent otherwise. */
  origin?: BaselineOrigin | null
}

/** The approved baseline a from-baseline scan starts from (only what the App reads). */
export interface BaselineOrigin {
  baseline: string
  commit: string
}

export type DiffKind = 'added' | 'removed' | 'renamed' | 'moved' | 'files' | 'endpoints' | 'commands' | 'content' | 'merged_into' | 'split_from'
export interface ObjectDiff {
  id: string
  object_type: string
  title: string
  kinds: DiffKind[]
  before_title?: string
  files_added?: string[]
  files_removed?: string[]
  /** The module merged into, or split from. */
  related?: string
}
/** J4: a draft entered from a baseline against that baseline; read only. */
export interface DraftComparison {
  current_baseline: string
  /** The draft started from a baseline that is no longer current. */
  outdated: boolean
  stale: boolean
  changed_paths: string[]
  impact: { intents: string[]; modules: string[]; interfaces: string[]; tasks: string[]; paths: string[] }
  comparison: { draft_id: string; baseline: string; commit: string; objects: ObjectDiff[]; unchanged: string[]; unassigned: string[] }
}

export interface CoverageChecks {
  coverage_ok: boolean
  module_count_ok: boolean
  commands_ok: boolean
  import_coverage: number
  imports: { supported_files: number; connected_files: number; resolved: number; external: number; skipped: number; languages: Record<string, number> }
  low_confidence: string[]
}

export interface ScanJob {
  id: string
  status: 'running' | 'ready' | 'failed' | 'cancelled'
  phase?: 'inventory' | 'queued' | 'verifying' | 'cancelling' | 'complete' | 'interrupted'
  cancel_requested?: boolean
  report?: ScanReport
  error?: string
}

export interface BaselineVerificationJob {
  id: string
  parent_id: string
  input_sha256: string
  status: 'running' | 'ready' | 'failed' | 'cancelled'
  phase: 'queued' | 'verifying' | 'cancelling' | 'publishing' | 'complete' | 'interrupted'
  cancel_requested: boolean
  result_id: string | null
  error: string | null
}

export interface DraftSummary {
  id: string
  producer: string
  created_at: string
  source_revision: string
  title: string
  stale: boolean
}

export interface DraftDocument {
  id: string
  path: string
  text: string
}

export interface ReviewCommand { directory: string; command: string }
export type ReviewAction =
  | { kind: 'rename'; node_id: string; title: string }
  | { kind: 'move'; node_id: string; x: number; y: number }
  | { kind: 'merge'; module_ids: string[]; target_id: string; title: string }
  | { kind: 'split'; module_id: string; new_id: string; title: string; files: string[] }
  | { kind: 'interface'; id: string; from: string; to: string; title: string; contract: string }
  | { kind: 'remove_interface'; id: string }
  | { kind: 'commands'; module_id: string; commands: ReviewCommand[] }
  | { kind: 'reference'; path: string; x: number; y: number }

export interface ReviewModule {
  id: string
  title: string
  files: string[]
  commands: ReviewCommand[]
  verification_pending: boolean
  commands_confirmed: boolean
}

export interface BaselineDraft {
  id: string
  producer: string
  self_reported: true
  created_at: string
  submission: {
    scan_id: string
    project_id: string
    source_revision: string
    intents: DraftDocument[]
    modules: DraftDocument[]
    interfaces: DraftDocument[]
    acceptances: DraftDocument[]
    questions: string[]
    risks: string[]
  }
  scan: ScanReport
  view: ProjectView
  review?: {
    base_id: string
    actor: string
    actions: ReviewAction[]
    pending_modules: string[]
    confirmed_modules: string[]
    modules: ReviewModule[]
    verification?: {
      parent_id: string
      input_sha256: string
      run_id: string
      limits: { command_timeout_ms: number; total_timeout_ms: number; output_limit_bytes: number }
      report: VerificationReport
    }
  }
  evidence_error?: string | null
  review_evidence_error?: string | null
  stale?: boolean
}

export interface ProjectState {
  /** `hub_offline`: the configured remote hub is unreachable (FR-88). */
  status: 'ready' | 'not_activated' | 'needs_recovery' | 'hub_offline'
  projection?: unknown
  error?: string
  /** Set on a ready state served from the last mirror while the hub is away. */
  warnings?: string[]
  seq?: number
  /** Derived from events on every read; null when it cannot be computed. */
  delegation_view?: DelegationView | null
  /** FR-128, FR-130: the person's budget and focus time from `config.yaml`; absent from older daemons. */
  attention?: { budget: number; focus_time: { start: number; end: number }[] } | null
  /** FR-134–136: due follow-ups and quiet or resurfaced objects; absent from older daemons. */
  quiet?: Rhythm | null
  /** Frames with bound cards and `frozen`, derived on every read; absent from older daemons. */
  frames?: FrameEntry[] | null
  /** FR-128, FR-135: the ordered attention list (`towermd_core::attention_list`); absent from older daemons. */
  attention_list?: AttentionListView | null
  /** FR-189: starter kits to recommend or already adopted; absent from older daemons. */
  starters?: StarterView | null
}

/** One ordered attention item as the daemon serves it; `label` is the kind the App shows. */
export interface AttentionEntry {
  id: string
  label: string
  title: string
  at: number
  objects: string[]
  areas: string[]
  object: string | null
  detail: string | null
  action: string
  urgent: boolean
  delegation: string | null
  signal: string | null
  class: 1 | 2 | 3 | 4
  timing: 'now' | 'next_visit'
  quiet: boolean
  card: string
}
/** Awake items first, then quiet ones; the first `shown` are the first screen. */
export interface AttentionListView { items: AttentionEntry[]; shown: number; asleep: number; budget: number }

/** FR-133, FR-39: one project's or container's health; the colour always comes with `symbol` and `label`. */
export interface Health {
  color: 'red' | 'yellow' | 'green' | 'grey'
  label: string
  symbol: string
  reasons: string[]
  done: number
  running: number
  blocked: number
  interrupts: Record<string, number>
  check: 'failed' | 'pending' | 'passed' | null
}
export interface ProjectHealth extends Health { areas: Record<string, Health> }
/** One project of this machine (`GET /overview`); states are null when the daemon could not read it. */
export interface OverviewProject {
  id: string
  root: string
  present: boolean
  status: string
  health: ProjectHealth | null
  needs: number | null
  last_activity: string | null
  error?: string
}
/** The cross-project attention list: at most three items, each tagged with its project. */
export interface CrossAttention {
  items: (AttentionEntry & { project: string; cmd: string })[]
  shown: number
  total: number
  later: number
  budget: number
}
export interface Overview { projects: OverviewProject[]; attention: CrossAttention | null }

// Delegation read model (FR-185, FR-201–211), as the daemon serializes it.
export interface Checkpoint { text: string; due?: string }
export interface Brief {
  why?: string; done_when: string[]; limits?: string; checkpoints?: Checkpoint[]; self_check?: string
}
export interface SelfReported { name: string; version: string; vendor?: string; model?: string; skills?: string }
export interface Stamp { at: string; eid: string; seq: number }
export interface Mark extends Stamp { actor: string }
export interface DelegationEntry<T> { event: Mark; data: T }
export type DelegationPhase = 'clarifying' | 'proposed' | 'ready' | 'running' | 'claimed_done' | 'done'
export type EvidenceSourceKind = 'check' | 'claimed' | 'used'
export interface DelegationEvidence {
  id: string; source_kind: EvidenceSourceKind; criteria: number[]; passed: boolean | null
  version: string | null; actor: string; at: string; note: string | null; attempt: string
  /** The check kind behind `check` evidence, e.g. `core.verify`. */
  check?: string
}
// Completion judgement (FR-120–124): kit checks plus `.towermd/gates.yaml`.
/** `waived`: a person let a failed or pending declared hard gate through until a time (FR-72). */
export type GateStatus = 'passed' | 'failed' | 'error' | 'pending' | 'clear' | 'waived'
export interface GateResult {
  gate: string; check: string | null; hard: boolean; builtin: boolean; status: GateStatus; reason?: string; evidence?: string[]
}
export interface Completion {
  verdict: 'passed' | 'pending' | 'blocked' | 'human_only'; version: string | null; gates: GateResult[]; error?: string
}
export interface Clarified { restated: string; done_when: string[]; questions?: string[]; self_reported?: SelfReported }
export interface ClarificationView { draft: Clarified; event: Mark; answers: string[] }
export interface VerdictView {
  kind: 'confirmed' | 'sent_back'; event: Mark; review_ms: number; unproven: number[]
  reason: string | null; takeaway: { text: string; set_by: string } | null
}
export interface AttemptView {
  id: string; config: string; delegate: SelfReported | null; delegated: Stamp; delivered: Stamp | null
  done: Stamp | null; done_by: 'check' | 'human' | null; active: boolean; version: string | null
  out_of_bounds: number | null; verdict: VerdictView | null
}
export interface AcceptanceCard {
  attempt: string; version: string | null
  rows: { index: number; text: string; evidence: DelegationEvidence[]; unproven: boolean }[]
  whole: DelegationEvidence[]; unproven: number[]; can_judge: boolean
}
export interface DelegationObject {
  id: string; title: string; phase: DelegationPhase; work_kind: string; default_work_kind: string
  brief: Brief; brief_set_by: string | null; clarification: ClarificationView | null
  human_can_complete: boolean; attempts: AttemptView[]; card: AcceptanceCard | null; completion: Completion
}
export interface Measure { count: number; median: number | null; total: number }
export interface Ratio { count: number; total: number; rate: number | null }
export interface TrackRecord {
  config: string; delegate: SelfReported | null; work_kind: string; records: number
  first_pass: Ratio; on_time: Ratio; loops: number | null; weakened: number | null; met_done_when: Ratio
  out_of_bounds: number | null; delegate_to_deliver_ms: Measure; review_ms: Measure; sent_back: Measure
  config_changed: boolean; spot_check_rate: number; level: string
  level_source: 'exact' | 'config' | 'work_kind' | 'project' | 'default'
  suggestion: { direction: 'up' | 'down'; level: string } | null
  /** FR-185: what the level lets a delegate do without asking; never a person's content. */
  allows: DelegationCapability[]
}
export type DelegationCapability = 'answer_by_decision' | 'approve_minor'
export interface ConfigHistory {
  name: string; configs: { config: string; delegate: SelfReported; first: Stamp; delegations: number }[]
}
export interface SpotCheck {
  object: string; attempt: string; config: string; work_kind: string; done: Stamp; rate: number; draw: number; resolved: boolean
}
export type ReportState = 'not_started' | 'in_progress' | 'waiting' | 'blocked' | 'claimed_done'
export interface ReportRequestView {
  id: string; object: string; attempt: string; checkpoint: number; text: string; due: string; open: boolean
  reply: DelegationEntry<{ state: ReportState; summary: string; next?: string; self_reported?: SelfReported }> | null
  overdue: DelegationEntry<{ level: 'active' | 'passive' }> | null
}
/** FR-116, FR-127: a person's report request or node question on any object. */
/** `update`: ask whoever writes a frame to update it by its rules (FR-156). */
export type AskKind = 'report' | 'question' | 'update'
export interface GenericRequestView {
  id: string; event: Mark
  data: { target: string; ask: AskKind; text: string; due: string; to: string[] }
  reply?: { event: Mark; data: { from: string; state: ReportState; summary: string; next?: string; refs?: string[]; self_reported?: SelfReported } } | null
  withdrawn?: Mark | null
  open: boolean
  /** Who may answer now; empty means queued until someone declares `answers`. */
  recipients: string[]
  /** The `core.silent` raised when it went past its deadline unanswered. */
  silence: Mark | null
}
export interface DelegationSettings {
  review_backlog: number; spot_check_min: number; track_record_window: number; delegation_levels: string[]
}
export interface DelegationView {
  objects: DelegationObject[]
  track_record: TrackRecord[]
  configs: ConfigHistory[]
  spot_checks: SpotCheck[]
  backlog: { count: number; threshold: number } | null
  report_requests: ReportRequestView[]
  /** Generic requests and node questions; absent from older daemons. */
  requests?: GenericRequestView[]
  rules: { id: string; work_kind: string; text: string; source: string | null; event: Mark }[]
  renames: Record<string, string>
  levels: { config: string; work_kind: string; level: string; event: Mark }[]
  settings: DelegationSettings
}

/** A person's delegation action: exact event body plus a fresh UUIDv7. */
export interface DelegationRequest<T> { request_id: string; object: string; data: T }
export interface LedgerRequest<T> { request_id: string; data: T }
/** "知道了" (`ack`) or "静音" (`mute`, with an end) on one merged signal; `seen` is its latest event. */
/** No recipients: the daemon fixes them. */
export interface AskInput { target: string; ask: AskKind; text: string; due: string }
export interface SignalDismissInput { from: string; kind: string; target: string; mode: 'ack' | 'mute'; seen: string; until?: string }
export interface DelegationReceipt { request_id: string; event: string; seq: number; status: 'accepted'; obj: string }
export interface FinalizeInput { brief: Brief; answers?: string[] }
export interface UsedInput { criteria: number[]; note?: string }
export interface ConfirmInput { review_ms: number; unproven: number[]; takeaway?: string }
export interface SendBackInput { reason: string; review_ms: number }
/** `kind` is the work kind the rule applies to; `source` the send-back event. */
export interface RuleInput { kind: string; text: string; source?: string }
/** FR-72: one declared hard gate on the object, until an RFC 3339 time at most 30 days away. */
export interface WaiveInput { gate: string; reason: string; until: string }
/** Spec 12.1: a person's way out of a held task, naming the interrupt they saw. There is no pass. */
export interface InterruptInput { choice: 'retry' | 'rewrite' | 'reassign'; interrupt: string; note?: string }
/** FR-58: a verdict on a G1/G4 review; asking for changes or rejecting needs a comment. */
export interface VerdictInput { verdict: 'passed' | 'changes_requested' | 'rejected'; comment?: string }
export interface RenameInput { from: string; to: string }
/** `config` is a fingerprint or `*`; `kind` a work kind or `*`. */
export interface LevelInput { config: string; kind: string; level: string }

export interface ProjectStreamMessage {
  subscription: string
  token: string
  status: 'connected' | 'disconnected'
  state?: ProjectState
}

export interface PublicationApproval {
  request_id: string
  target: string
  prepared_at: string
  content_hash: string
}

export interface PublicationPreview extends PublicationApproval {
  draft_id: string
  project_id: string
  source_revision: string
}

export interface PublicationReceipt {
  target: string
  commit: string
  tag: string
  content_hash: string
  snapshot_commit: string
}

export interface RecoveryReceipt {
  status: 'ready'
  snapshot_commit: string
  seq: number
}

export type PromptScope = { kind: 'global' } | { kind: 'module'; module: string } | { kind: 'task'; task: string }
export interface AnswerRequest {
  request_id: string
  hold: string
  text: string
  remember_as: 'temporary' | 'module_decision'
  targets: string[]
  /** FR-70 `/assume`: one line, delivered with the answer. */
  assumption?: string
  /** FR-186: the day (`YYYY-MM-DD`, UTC) to look at a module decision again. */
  review_on?: string
}
/** FR-186: keep a decision that came due (optionally to a later day) or void it. */
export interface DecisionReviewRequest { request_id: string; decision: string; outcome: 'keep' | 'void'; review_on?: string }
export interface PromptRequest {
  request_id: string
  scope: PromptScope
  text: string
  targets: string[]
}
export interface HumanReceipt {
  request_id: string
  event: string
  seq: number
  status: 'accepted'
  proposal: string | null
}
export type MainResolutionOperation = 'claim' | 'revert'
export interface MainResolutionPreview {
  version: 1
  request_id: string
  operation: MainResolutionOperation
  project_id: string
  docs_dir: string
  import_anchor: string
  main_revision: string
  target_commit: string
  parent_commit: string
  changes: { path: string; status: string; old_mode: string; new_mode: string; old_oid: string; new_oid: string }[]
  content_hash: string
  diff: string
}
export interface MainResolutionReceipt {
  request_id: string
  event: string
  seq: number
  operation: MainResolutionOperation
  target_commit: string
  resolution_commit: string
  snapshot_commit: string
}
export interface RunDetails {
  run_id: string
  task_id: string
  seq: number
  messages: { kind: 'question' | 'answer' | 'prompt'; id: string; text: string; at: string; actor: string }[]
  events: { eid: string; seq: number; at: string; actor: string; kind: string; object: string; stale: boolean; evidence_id: string | null }[]
  evidence: { id: string; completion_eid: string; revision: string; tree: string; passed: boolean; report_sha256: string; report: VerificationReport }[]
  harness_output: null
}
export interface ProposalEntry {
  type: 'Proposal'
  id: string
  status: 'draft' | 'submitted' | 'approved' | 'rejected'
  event: string
  text?: string
  targets?: unknown[]
  baseline?: string
  approval_eid?: string
  rejection?: { eid: string; reason: string }
  /** FR-125: what this proposal would weaken, judged by the daemon; `[]` when nothing decidable got looser. */
  weakening?: { change: string; text: string }[]
  proposal?: {
    id: string; project_id: string; source_revision: string; baseline_tag: string
    draft_id: string | null; source_proposal_id: string | null; source_commit: string | null
    kind: string; reason: string; alternatives: string[]; recommendation: string
    impact: { intents: string[]; modules: string[]; interfaces: string[]; tasks: string[]; paths: string[] }
    changes: { path: string; before_oid: string | null; after_oid: string | null; before: string | null; after: string | null }[]
    content_hash: string
  }
}
export interface ProposalRejection { request_id: string; proposal_hash: string; reason: string }
export interface HumanTaskDraft {
  id: string; title: string; module: string; allowed_paths: string[]
  touches: string[]; acceptance: string[]; depends_on: string[]; decisions: string[]; priority: number
  why?: string; limits?: string; checkpoints?: Checkpoint[]; self_check?: string; clarify?: boolean
}
export interface ProjectControlState {
  paused: boolean
  pause_event: string | null
  scope: 'all'
  resume_requests: { event: string; pause_event: string; actor: string; at: string }[]
}
export type ProjectControlRequest = { kind: 'pause'; request_id: string }
  | { kind: 'resume'; request_id: string; pause_event: string }
export interface ControlReceipt {
  request_id: string; event: string; seq: number; status: 'accepted'; control: ProjectControlState
}
/** FR-183: an idea is not a commitment until a person promotes it. */
export interface IdeaMark { at: string; eid: string; seq: number; actor: string }
export interface IdeaEntry {
  id: string; text: string; added: IdeaMark
  promoted?: { as: 'project' | 'task' | string; target?: string | null; event: IdeaMark }
}
export interface IdeaRequest { request_id: string; text: string }
export type IdeaPromotion = { request_id: string; idea: string; as: 'project' }
  | { request_id: string; idea: string; as: 'task'; task: HumanTaskDraft }
export interface IdeaReceipt { request_id: string; event: string; seq: number; status: 'accepted'; idea: IdeaEntry }
/** FR-134: the condition that makes a follow-up due. */
export type FollowUpWhen = { on: 'date'; at: string } | { on: 'done'; target: string }
  | { on: 'activity'; target: string } | { on: 'signal'; target: string; kind?: string }
export interface FollowUpEntry { id: string; text: string; when: FollowUpWhen; added: Mark; done?: Mark }
export interface FollowUpRequest { request_id: string; text: string; when: FollowUpWhen }
export interface FollowUpReceipt { request_id: string; event: string; seq: number; status: 'accepted'; follow_up: FollowUpEntry | null }
/** FR-69: a person's temporary rule; it stops applying at `until`. */
export interface TempRuleEntry { id: string; text: string; scope: PromptScope; until: string; added: Mark; expired?: Mark | null }
export interface TempRuleRequest { request_id: string; text: string; scope: PromptScope; until: string }
export interface TempRuleReceipt { request_id: string; event: string; seq: number; status: 'accepted'; rule: TempRuleEntry }
/** FR-136: why a quiet object came back. */
export type Resurfaced = { by: 'follow_up'; follow_up: string } | { by: 'check_every'; days: number } | { by: 'connector'; event: Mark }
export interface ObjectRhythm { id: string; last: Mark | null; quiet: boolean; resurfaced: Resurfaced | null }
/** FR-134–136, derived by the daemon on every read: the project first, then each object. */
export interface Rhythm { dormant_after: number; objects: ObjectRhythm[]; due: string[] }
export interface HumanTaskReceipt {
  request_id: string; event: string; seq: number; status: 'accepted'; task: { input: HumanTaskDraft; origin: 'app_human' }
}
/** FR-141–146: a template's layout as the daemon checked it. */
export type FrameAxis = 'none' | 'order' | { category: { field: string; values: string[] } } | { time: { field: string } }
export interface FrameLayout { x: FrameAxis; y: FrameAxis; structure: ('nesting' | 'relations' | 'tree')[] }
export interface FrameLimits { title_max: number; body_max: number; per_lane_max?: number }
export type FrameWho = 'human' | 'anyone'
export interface FrameTemplate {
  id: string; origin: 'starter' | 'installed'; path: string; version?: string | null; title?: string | null
  layout?: FrameLayout; states?: string[]; done_states?: string[]; card_fields?: Record<string, 'text' | 'date' | 'ref' | 'number'>
  limits?: FrameLimits; default_set_by?: 'human' | 'delegate'; problems: string[]
  /** FR-163: a text-source template asks for a repository file of these types. */
  file_types?: string[]
}
export interface FramePlace { x: number; y: number; width: number; height: number }
/** FR-146/153: what a person may narrow when placing a frame; the daemon fills the rest from the template. */
export interface FrameRulesInput {
  text?: string; add_card?: FrameWho; add_link?: FrameWho; edit_text?: FrameWho; change_state?: FrameWho
  fields?: string[]; transitions?: Record<string, string[]>; per_lane_max?: number; require_source?: boolean; done_needs_human?: boolean
  /** FR-162: past this date (inclusive, UTC) or once this object is done, the frame takes no writes. */
  update_until?: FrameUntil
  /** FR-156: look again every this many days (1–365); feeds dormancy and resurfacing. */
  review_every?: number
  /** FR-161: no callouts beside this frame's cards. */
  callouts_off?: boolean
}
export type FrameUntil = { date: string } | { object: string }
export interface FrameRules extends Required<Omit<FrameRulesInput, 'per_lane_max' | 'update_until' | 'review_every' | 'callouts_off'>> {
  per_lane_max?: number; update_until?: FrameUntil; review_every?: number; callouts_off?: boolean
}
/** FR-147: one card operation, the body of the `card` verb and of a person's write. */
export type CardOp =
  | { op: 'add'; title: string; body?: string; state?: string; fields?: Record<string, unknown>; parent?: string }
  | { op: 'update'; title?: string; body?: string; fields?: Record<string, unknown> }
  | { op: 'move'; state: string }
  | { op: 'link'; to: string }
  | { op: 'archive' }
  | { op: 'rules'; rules: FrameRulesInput }
  /** FR-164/165: an edit of a text-source frame's file; the writer patches the lines on main. */
  | { op: 'source'; change: SourceChange }
/** FR-164: one edit of a text-source frame's file, keyed by element (alias or name). */
export type SourceChange =
  | { do: 'add'; title: string; alias?: string; parent?: string; state?: string }
  | { do: 'title'; element: string; title: string }
  | { do: 'state'; element: string; state: string }
  | { do: 'link'; from: string; to: string; label?: string; dashed?: boolean }
  | { do: 'note'; element: string; kind: 'why' | 'adjust' | 'question'; text: string }
  /** FR-175: a repair the person saw, bound to the file it was offered for. */
  | { do: 'fix'; fix: string; sha256: string }
/** One element of a text-source frame's file, as parsed on main. */
export interface SourceCard {
  key: string; title: string; keyword: string; parent?: string; state?: string; done: boolean
  links?: string[]; body?: string[]; line: number; diagram: number; implicit?: boolean
  /** FR-176: why canvas edits of this element are off (a macro, an included file, a clashing alias). */
  read_only?: string
}
export interface SourceLine { line: number; text: string; reason: string }
export interface SourceFix { id: string; title: string; diff: string[] }
/** FR-163, FR-174–177: a text-source frame's file, parsed again on each read and never stored. */
export interface FrameSourceView {
  path: string; status?: 'ok' | 'unparsable' | 'missing' | 'too_large' | 'not_utf8'
  revision?: string; shown_from?: string; locked?: boolean; renamed_to?: string
  cards?: SourceCard[]
  links?: { from: string; to: string; label?: string; dashed?: boolean; line: number; read_only?: string }[]
  callouts?: { kind: 'why' | 'adjust' | 'question'; target?: string; text: string; full: string; line: number }[]
  unknown?: SourceLine[]; unknown_lines?: number; understood?: number
  problems?: SourceLine[]; fixes?: SourceFix[]; includes?: string[]
  lint?: { code: string; text: string; lines?: number[] }[]
  signal?: { level: 'passive' | 'active'; text: string }
  sha256?: string
  /** The daemon could not read the file at all. */
  error?: string
}
/** FR-169: one commit of the file, summarized from the parses alone. */
export interface SourceStep { commit: string; at: string; author: string; subject: string; summary: { text: string } & Record<string, unknown> }
export interface SourceHistory { frame: string; path: string; main: string; steps: SourceStep[] }
export interface SourceAt { frame: string; source: FrameSourceView }
export interface FrameCard {
  id: string; title: string; body?: string; state?: string | null; fields?: Record<string, unknown>
  parent?: string | null; links?: string[]; human?: string[]; archived?: boolean
  added: IdeaMark; changed: IdeaMark; by?: string | null
}
/**
 * proposal: a frame a person set; suggestion: over what a person set; confirm: into a done state; rules: a rule change;
 * write: a text-source edit queued for the writer, with `note` once the file no longer takes it.
 */
export type FramePendingWhy = 'proposal' | 'suggestion' | 'confirm' | 'rules' | 'write'
export interface FramePending { id: number; why: FramePendingWhy; target: string; op: CardOp; from?: string | null; by: IdeaMark; note?: string }
/** FR-148: a card that follows a core object, derived on every read. */
export interface FrameBound { id: string; title: string; state: string; phase: string; done: boolean }
export interface FrameEntry {
  id: string; title: string
  template: { id: string; version: string; definition: unknown }
  layout: FrameLayout; states: string[]; done_states: string[]; limits: FrameLimits
  set_by: 'human' | 'delegate'; mode: 'bound' | 'free' | 'mixed'; rules: FrameRules; place: FramePlace
  created: IdeaMark; placed?: IdeaMark | null
  objects?: string[]; cards?: FrameCard[]; pending?: FramePending[]; rules_changed?: IdeaMark | null
  /** FR-149: cards a person dragged, in frame coordinates. */
  pins?: Record<string, { x: number; y: number }>
  /** From the daemon's `frames` view: bound cards and whether `update_until` has passed. */
  bound?: FrameBound[]; frozen?: boolean
  /** FR-160: derived notes beside the frame or its cards; never stored, never laid out. */
  callouts?: FrameCallout[]
  /** FR-154: the last time a person wrote this frame into `tower.canvas`. */
  materialized?: { commit: string; sha256: string; at: IdeaMark } | null
  /** FR-163: a text-source frame's file; the daemon's `frames` view replaces the stored path with the file as parsed on main. */
  source?: FrameSourceView | null
}
/** why: a remembered decision; adjustment: an answer or prompt; question: still open. At most 100 characters. */
export type CalloutKind = 'why' | 'adjustment' | 'question'
export interface FrameCallout { kind: CalloutKind; target: string; text: string; source: string }
/** FR-154: what confirming would write, for the person to read first. */
export interface MaterializePreview {
  version: number; request_id: string; project_id: string; docs_dir: string; frame: string; seq: number
  main_revision: string; sha256: string; nodes: string[]; canvas: string; content_hash: string
}
export interface MaterializeReceipt {
  request_id: string; event: string; seq: number; status: 'accepted'; frame: string; commit: string; snapshot_commit: string | null
}
/** FR-157/158: one step in a frame's history and what it changed per card. */
export type CardChange = 'new' | 'state' | 'text'
export interface FrameStep { seq: number; at: string; kind: string; actor: string; cards: Record<string, CardChange[]> }
export interface FrameHistory { frame: string; seq: number; steps: FrameStep[] }
export interface FrameHistoryAt { frame: string; seq: number; step: FrameStep; view: FrameEntry[] }
/** FR-189: starter kits; recommended once after the first content, never stored as recommended. */
export interface StarterKit { id: string; title: string; templates: string[]; checks: string[]; names: Record<string, string> }
export interface StarterView { recommended: StarterKit[]; adopted: StarterKit[]; names: Record<string, string> }
export interface StarterRequest { request_id: string; kit: string; adopt: boolean }
export interface StarterReceipt { request_id: string; event: string; seq: number; status: 'accepted'; starter: string; starters: StarterView | null }
/** No definition, signer or layer: the daemon reads the template itself and signs `human:<id>`. */
export interface FrameRequest {
  request_id: string; id: string; title: string; template: string
  set_by?: 'human' | 'delegate'; rules?: FrameRulesInput; place: FramePlace
  mode?: 'bound' | 'free' | 'mixed'; objects?: string[]
  /** FR-163: the file a text-source frame shows; `create` has the writer start it on main. */
  source?: { path: string; create?: boolean }
  /** FR-144: pasted Markdown or Mermaid; the daemon reads it into the new free frame's first cards. */
  text?: string
}
/** FR-155: the frame as text, derived on every read. */
export type FrameFormat = 'markdown' | 'mermaid'
export interface FrameExport { frame: string; format: FrameFormat; formats: FrameFormat[]; text: string }
/** FR-159: a view derived from time, computed from the events on every read. */
export type FrameViewName = 'cumulative_flow'
/** Each day's count per state at the end of that day (UTC), in the order of `states`. */
export interface FrameFlow { frame: string; states: string[]; done_states: string[]; days: { date: string; counts: number[] }[] }
export interface FrameViewRead { view: FrameViewName; flow: FrameFlow }
export interface RelinkSourceRequest { request_id: string; frame: string; path: string }
export interface PlaceFrameRequest { request_id: string; frame: string; place: FramePlace }
export interface FrameReceipt { request_id: string; event: string; seq: number; status: 'accepted'; frame: FrameEntry }
/** A person's card write; `target` is the card, or the frame when adding. */
export interface CardRequest { request_id: string; frame: string; target?: string; op: Exclude<CardOp, { op: 'rules' }> }
export interface FrameRulesRequest { request_id: string; frame: string; rules: FrameRules }
export interface FrameDecisionRequest { request_id: string; frame: string; pending: number; accept: boolean }
/** FR-149: `null` lets a card go back to the derived layout. */
export interface PinCardsRequest { request_id: string; frame: string; pins: Record<string, { x: number; y: number } | null> }
export interface CardReceipt extends FrameReceipt {
  receipt: { status: 'applied' | 'partly_suggested' | 'proposed' | 'suggested' | 'awaiting_confirmation' | 'queued'; card: string | null; pending: { id: number; why: FramePendingWhy; target: string }[]; hint: string | null }
  /** FR-164: what the single main writer did with a text-source frame's queued writes. */
  source_write?: SourceWrite | null
}
export interface SourceWrite { written: { pending: number; path: string; commit?: string; sha256?: string; failed?: string }[]; error?: string }

export interface ProjectClient {
  chooseProject(): Promise<ProjectSelection | null>
  /** FR-133: every project this machine knows, with health and one attention list. */
  overview?(): Promise<Overview>
  /** Opens a project the overview lists as present, by ID only. */
  openKnownProject?(id: string): Promise<ProjectSelection | null>
  loadProject(token: string): Promise<ProjectView>
  /** `fromBaseline` seeds the scan from the current approved baseline (J4). */
  startScan(token: string, fromBaseline?: boolean): Promise<ScanJob>
  pollScan(token: string, id: string): Promise<ScanJob>
  cancelScan(token: string, id: string): Promise<ScanJob>
  listBaselines?(token: string): Promise<DraftSummary[]>
  draftFromScan?(token: string, id: string): Promise<BaselineDraft>
  compareBaseline?(token: string, id: string): Promise<DraftComparison>
  loadBaseline?(token: string, id: string): Promise<BaselineDraft>
  reviewBaseline?(token: string, id: string, action: ReviewAction): Promise<BaselineDraft>
  verifyBaseline?(token: string, id: string, requestId: string): Promise<BaselineVerificationJob>
  pollBaselineVerification?(token: string, id: string): Promise<BaselineVerificationJob>
  cancelBaselineVerification?(token: string, id: string): Promise<BaselineVerificationJob>
  projectState?(token: string): Promise<ProjectState>
  subscribeProject?(token: string, subscriptionId: string, onMessage: (message: ProjectStreamMessage) => void): Promise<void>
  unsubscribeProject?(subscriptionId: string): Promise<void>
  preparePublication?(token: string, id: string): Promise<PublicationPreview>
  approvePublication?(token: string, id: string, approval: PublicationApproval): Promise<PublicationReceipt>
  requestRecovery?(token: string): Promise<{ request_id: string }>
  confirmRecovery?(token: string, id: string): Promise<RecoveryReceipt>
  answerQuestion?(token: string, input: AnswerRequest): Promise<HumanReceipt>
  sendPrompt?(token: string, input: PromptRequest): Promise<HumanReceipt>
  createHumanTask?(token: string, input: { request_id: string; task: HumanTaskDraft }): Promise<HumanTaskReceipt>
  taskOptions?(token: string): Promise<{ baseline: string; modules: { id: string; title: string; paths: string[] }[] }>
  controlProject?(token: string, input: ProjectControlRequest): Promise<ControlReceipt>
  prepareMainResolution?(token: string, operation: MainResolutionOperation, commit: string): Promise<MainResolutionPreview>
  confirmMainResolution?(token: string, preview: MainResolutionPreview): Promise<MainResolutionReceipt>
  runDetails?(token: string, id: string): Promise<RunDetails>
  listProposals?(token: string): Promise<{ proposals: ProposalEntry[] }>
  prepareProposal?(token: string, id: string): Promise<PublicationPreview>
  approveProposal?(token: string, id: string, approval: PublicationApproval): Promise<PublicationReceipt>
  rejectProposal?(token: string, id: string, input: ProposalRejection): Promise<ProposalEntry>
  captureMainProposal?(token: string, commit: string): Promise<ProposalEntry>
  referencePath?(token: string, path: string): Promise<string>
  finalizeDelegation?(token: string, input: DelegationRequest<FinalizeInput>): Promise<DelegationReceipt>
  recordAcceptanceUsed?(token: string, input: DelegationRequest<UsedInput>): Promise<DelegationReceipt>
  confirmAcceptance?(token: string, input: DelegationRequest<ConfirmInput>): Promise<DelegationReceipt>
  sendBackAcceptance?(token: string, input: DelegationRequest<SendBackInput>): Promise<DelegationReceipt>
  /** `object` is the rule ID. */
  recordRule?(token: string, input: DelegationRequest<RuleInput>): Promise<DelegationReceipt>
  /** `object` is the waived object; the receipt's event is the waiver's ID. */
  waiveGate?(token: string, input: DelegationRequest<WaiveInput>): Promise<DelegationReceipt>
  /** `object` is the held task. */
  resolveInterrupt?(token: string, input: DelegationRequest<InterruptInput>): Promise<DelegationReceipt>
  /** `object` is the review ID (`V-…`). */
  resolveReview?(token: string, input: DelegationRequest<VerdictInput>): Promise<DelegationReceipt>
  renameWorkKind?(token: string, input: LedgerRequest<RenameInput>): Promise<DelegationReceipt>
  setDelegationLevel?(token: string, input: LedgerRequest<LevelInput>): Promise<DelegationReceipt>
  dismissSignal?(token: string, input: LedgerRequest<SignalDismissInput>): Promise<DelegationReceipt>
  askAbout?(token: string, input: LedgerRequest<AskInput>): Promise<DelegationReceipt>
  withdrawRequest?(token: string, input: DelegationRequest<Record<string, never>>): Promise<DelegationReceipt>
  addIdea?(token: string, input: IdeaRequest): Promise<IdeaReceipt>
  promoteIdea?(token: string, input: IdeaPromotion): Promise<IdeaReceipt>
  addFollowUp?(token: string, input: FollowUpRequest): Promise<FollowUpReceipt>
  finishFollowUp?(token: string, input: { request_id: string; follow_up: string }): Promise<FollowUpReceipt>
  addTempRule?(token: string, input: TempRuleRequest): Promise<TempRuleReceipt>
  /** FR-136 "看过了" on a resurfaced object. */
  seeResurfaced?(token: string, input: { request_id: string; object: string }): Promise<{ request_id: string; event: string; seq: number; status: 'accepted'; object: string }>
  reviewDecision?(token: string, input: DecisionReviewRequest): Promise<{ request_id: string; event: string; seq: number; status: 'accepted'; review: unknown }>
  /** FR-130: a system notification for an item that interrupts now. Only the App notifies; the daemon never does. */
  notify?(title: string, body: string): Promise<void>
  listTemplates?(token: string): Promise<{ revision: string; templates: FrameTemplate[] }>
  createFrame?(token: string, input: FrameRequest): Promise<FrameReceipt>
  placeFrame?(token: string, input: PlaceFrameRequest): Promise<FrameReceipt>
  writeCard?(token: string, input: CardRequest): Promise<CardReceipt>
  changeFrameRules?(token: string, input: FrameRulesRequest): Promise<CardReceipt>
  resolveFrameItem?(token: string, input: FrameDecisionRequest): Promise<CardReceipt>
  pinCards?(token: string, input: PinCardsRequest): Promise<CardReceipt>
  prepareMaterialize?(token: string, frame: string): Promise<MaterializePreview>
  confirmMaterialize?(token: string, preview: MaterializePreview): Promise<MaterializeReceipt>
  frameHistory?(token: string, frame: string): Promise<FrameHistory>
  frameHistoryAt?(token: string, frame: string, seq: number): Promise<FrameHistoryAt>
  frameSourceHistory?(token: string, frame: string): Promise<SourceHistory>
  frameSourceAt?(token: string, frame: string, commit: string): Promise<SourceAt>
  relinkFrameSource?(token: string, input: RelinkSourceRequest): Promise<FrameReceipt>
  exportFrame?(token: string, frame: string, format?: FrameFormat): Promise<FrameExport>
  frameView?(token: string, frame: string, view: FrameViewName): Promise<FrameViewRead>
  chooseStarter?(token: string, input: StarterRequest): Promise<StarterReceipt>
}

export const projectClient: ProjectClient = {
  chooseProject: () => invoke<ProjectSelection | null>('choose_project'),
  overview: () => invoke<Overview>('overview'),
  openKnownProject: (id) => invoke<ProjectSelection | null>('open_known_project', { id }),
  loadProject: (token) => invoke<ProjectView>('load_project', { token }),
  startScan: (token, fromBaseline) => invoke<ScanJob>('start_scan', fromBaseline ? { token, fromBaseline: true } : { token }),
  draftFromScan: (token, id) => invoke<BaselineDraft>('draft_from_scan', { token, id }),
  compareBaseline: (token, id) => invoke<DraftComparison>('compare_baseline', { token, id }),
  pollScan: (token, id) => invoke<ScanJob>('poll_scan', { token, id }),
  cancelScan: (token, id) => invoke<ScanJob>('cancel_scan', { token, id }),
  listBaselines: (token) => invoke<DraftSummary[]>('list_baselines', { token }),
  loadBaseline: (token, id) => invoke<BaselineDraft>('load_baseline', { token, id }),
  reviewBaseline: (token, id, action) => invoke<BaselineDraft>('review_baseline', { token, id, action }),
  verifyBaseline: (token, id, requestId) => invoke<BaselineVerificationJob>('verify_baseline', { token, id, requestId }),
  pollBaselineVerification: (token, id) => invoke<BaselineVerificationJob>('poll_baseline_verification', { token, id }),
  cancelBaselineVerification: (token, id) => invoke<BaselineVerificationJob>('cancel_baseline_verification', { token, id }),
  projectState: (token) => invoke<ProjectState>('project_state', { token }),
  subscribeProject: (token, subscriptionId, onMessage) => {
    const channel = new Channel<ProjectStreamMessage>()
    channel.onmessage = onMessage
    return invoke<void>('subscribe_project', { token, subscriptionId, onMessage: channel })
  },
  unsubscribeProject: (subscriptionId) => invoke<void>('unsubscribe_project', { subscriptionId }),
  preparePublication: (token, id) => invoke<PublicationPreview>('prepare_publication', { token, id }),
  approvePublication: (token, id, approval) => invoke<PublicationReceipt>('approve_publication', { token, id, approval }),
  requestRecovery: (token) => invoke<{ request_id: string }>('request_recovery', { token }),
  confirmRecovery: (token, id) => invoke<RecoveryReceipt>('confirm_recovery', { token, id }),
  answerQuestion: (token, input) => invoke<HumanReceipt>('answer_question', { token, input }),
  sendPrompt: (token, input) => invoke<HumanReceipt>('send_prompt', { token, input }),
  createHumanTask: (token, input) => invoke<HumanTaskReceipt>('create_human_task', { token, input }),
  taskOptions: (token) => invoke('task_options', { token }),
  controlProject: (token, input) => invoke<ControlReceipt>('control_project', { token, input }),
  prepareMainResolution: (token, operation, commit) => invoke<MainResolutionPreview>('prepare_main_resolution', { token, operation, commit }),
  confirmMainResolution: (token, preview) => invoke<MainResolutionReceipt>('confirm_main_resolution', { token, preview }),
  runDetails: (token, id) => invoke<RunDetails>('run_details', { token, id }),
  listProposals: (token) => invoke<{ proposals: ProposalEntry[] }>('list_proposals', { token }),
  prepareProposal: (token, id) => invoke<PublicationPreview>('prepare_proposal', { token, id }),
  approveProposal: (token, id, approval) => invoke<PublicationReceipt>('approve_proposal', { token, id, approval }),
  rejectProposal: (token, id, input) => invoke('reject_proposal', { token, id, input }),
  captureMainProposal: (token, commit) => invoke<ProposalEntry>('capture_main_proposal', { token, commit }),
  referencePath: (token, path) => invoke<string>('reference_path', { token, path }),
  finalizeDelegation: (token, input) => invoke<DelegationReceipt>('finalize_delegation', { token, input }),
  recordAcceptanceUsed: (token, input) => invoke<DelegationReceipt>('record_acceptance_used', { token, input }),
  confirmAcceptance: (token, input) => invoke<DelegationReceipt>('confirm_acceptance', { token, input }),
  sendBackAcceptance: (token, input) => invoke<DelegationReceipt>('send_back_acceptance', { token, input }),
  recordRule: (token, input) => invoke<DelegationReceipt>('record_rule', { token, input }),
  waiveGate: (token, input) => invoke<DelegationReceipt>('waive_gate', { token, input }),
  resolveInterrupt: (token, input) => invoke<DelegationReceipt>('resolve_interrupt', { token, input }),
  resolveReview: (token, input) => invoke<DelegationReceipt>('resolve_review', { token, input }),
  renameWorkKind: (token, input) => invoke<DelegationReceipt>('rename_work_kind', { token, input }),
  setDelegationLevel: (token, input) => invoke<DelegationReceipt>('set_delegation_level', { token, input }),
  dismissSignal: (token, input) => invoke<DelegationReceipt>('dismiss_signal', { token, input }),
  askAbout: (token, input) => invoke<DelegationReceipt>('ask_about', { token, input }),
  withdrawRequest: (token, input) => invoke<DelegationReceipt>('withdraw_request', { token, input }),
  addIdea: (token, input) => invoke<IdeaReceipt>('add_idea', { token, input }),
  promoteIdea: (token, input) => invoke<IdeaReceipt>('promote_idea', { token, input }),
  addFollowUp: (token, input) => invoke<FollowUpReceipt>('add_follow_up', { token, input }),
  finishFollowUp: (token, input) => invoke<FollowUpReceipt>('finish_follow_up', { token, input }),
  addTempRule: (token, input) => invoke<TempRuleReceipt>('add_temp_rule', { token, input }),
  seeResurfaced: (token, input) => invoke('see_resurfaced', { token, input }),
  reviewDecision: (token, input) => invoke('review_decision', { token, input }),
  notify: (title, body) => invoke<void>('notify_attention', { title, body }),
  listTemplates: (token) => invoke('list_templates', { token }),
  createFrame: (token, input) => invoke<FrameReceipt>('create_frame', { token, input }),
  placeFrame: (token, input) => invoke<FrameReceipt>('place_frame', { token, input }),
  writeCard: (token, input) => invoke<CardReceipt>('write_card', { token, input }),
  changeFrameRules: (token, input) => invoke<CardReceipt>('change_frame_rules', { token, input }),
  resolveFrameItem: (token, input) => invoke<CardReceipt>('resolve_frame_item', { token, input }),
  pinCards: (token, input) => invoke<CardReceipt>('pin_cards', { token, input }),
  prepareMaterialize: (token, frame) => invoke<MaterializePreview>('prepare_materialize', { token, frame }),
  confirmMaterialize: (token, preview) => invoke<MaterializeReceipt>('confirm_materialize', { token, preview }),
  frameHistory: (token, frame) => invoke<FrameHistory>('frame_history', { token, frame, seq: null }),
  frameHistoryAt: (token, frame, seq) => invoke<FrameHistoryAt>('frame_history', { token, frame, seq }),
  frameSourceHistory: (token, frame) => invoke<SourceHistory>('frame_source_history', { token, frame }),
  frameSourceAt: (token, frame, commit) => invoke<SourceAt>('frame_source', { token, frame, at: commit }),
  relinkFrameSource: (token, input) => invoke<FrameReceipt>('relink_frame_source', { token, input }),
  exportFrame: (token, frame, format) => invoke<FrameExport>('export_frame', { token, frame, format: format ?? null }),
  frameView: (token, frame, view) => invoke<FrameViewRead>('frame_view', { token, frame, view }),
  chooseStarter: (token, input) => invoke<StarterReceipt>('choose_starter', { token, input }),
}
