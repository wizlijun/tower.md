import { invoke } from '@tauri-apps/api/core'

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
  preparePublication?(token: string, id: string): Promise<PublicationPreview>
  approvePublication?(token: string, id: string, approval: PublicationApproval): Promise<PublicationReceipt>
  requestRecovery?(token: string): Promise<{ request_id: string }>
  confirmRecovery?(token: string, id: string): Promise<RecoveryReceipt>
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
  preparePublication: (token, id) => invoke<PublicationPreview>('prepare_publication', { token, id }),
  approvePublication: (token, id, approval) => invoke<PublicationReceipt>('approve_publication', { token, id, approval }),
  requestRecovery: (token) => invoke<{ request_id: string }>('request_recovery', { token }),
  confirmRecovery: (token, id) => invoke<RecoveryReceipt>('confirm_recovery', { token, id }),
  referencePath: (token, path) => invoke<string>('reference_path', { token, path }),
}
