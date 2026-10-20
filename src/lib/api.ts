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

export interface ProjectClient {
  chooseProject(): Promise<ProjectSelection | null>
  loadProject(token: string): Promise<ProjectView>
  startScan(token: string): Promise<ScanJob>
  pollScan(token: string, id: string): Promise<ScanJob>
  cancelScan(token: string, id: string): Promise<ScanJob>
}

export const projectClient: ProjectClient = {
  chooseProject: () => invoke<ProjectSelection | null>('choose_project'),
  loadProject: (token) => invoke<ProjectView>('load_project', { token }),
  startScan: (token) => invoke<ScanJob>('start_scan', { token }),
  pollScan: (token, id) => invoke<ScanJob>('poll_scan', { token, id }),
  cancelScan: (token, id) => invoke<ScanJob>('cancel_scan', { token, id }),
}
