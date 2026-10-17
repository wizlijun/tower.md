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
