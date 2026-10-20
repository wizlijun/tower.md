import { Channel, invoke } from '@tauri-apps/api/core'

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
  verification?: VerificationReport | null
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
  status: 'ready' | 'not_activated' | 'needs_recovery'
  projection?: unknown
  error?: string
  seq?: number
}

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
}
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
export interface HumanTaskReceipt {
  request_id: string; event: string; seq: number; status: 'accepted'; task: { input: HumanTaskDraft; origin: 'app_human' }
}

export interface ProjectClient {
  chooseProject(): Promise<ProjectSelection | null>
  loadProject(token: string): Promise<ProjectView>
  startScan(token: string): Promise<ScanJob>
  pollScan(token: string, id: string): Promise<ScanJob>
  cancelScan(token: string, id: string): Promise<ScanJob>
  listBaselines?(token: string): Promise<DraftSummary[]>
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
}

export const projectClient: ProjectClient = {
  chooseProject: () => invoke<ProjectSelection | null>('choose_project'),
  loadProject: (token) => invoke<ProjectView>('load_project', { token }),
  startScan: (token) => invoke<ScanJob>('start_scan', { token }),
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
}
