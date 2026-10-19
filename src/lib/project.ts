import { invoke } from '@tauri-apps/api/core';

export interface SelectedProject { token: string; displayPath: string; hasConfig: boolean }
export interface InitOutput { stdout: string; stderr: string; exitCode: number | null }
export interface AppError { code: string; message: string; detail: string | null }
export function chooseProject(): Promise<SelectedProject | null> {
  return invoke<SelectedProject | null>('choose_project');
}
export function initializeProject(token: string, docsDir: string): Promise<InitOutput> {
  return invoke<InitOutput>('initialize_project', { token, docsDir });
}

export interface ProjectOverview {
  source: 'baseline' | 'repository';
  revision: string;
  title: string;
  nodes: { id: string; title: string; kind: string; obj?: string; objectType?: string; status: string; text: string; x: number; y: number; width: number; height: number }[];
  edges: { from: string; to: string }[];
  diagnostics: string[];
}
export function loadProjectOverview(token: string): Promise<ProjectOverview> {
  return invoke<ProjectOverview>('load_project_overview', { token });
}
