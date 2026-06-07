import { writable, derived } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

export type View = 'welcome' | 'dashboard' | 'search' | 'documents' | 'connectors' | 'embeddings' | 'agents' | 'registry' | 'settings';

export interface WorkspaceInfo {
  name: string;
  path: string;
  document_count: number;
  chunk_count: number;
  embedded_chunk_count: number;
  last_sync: string | null;
  server_running: boolean;
}

export interface RecentWorkspace {
  name: string;
  path: string;
  last_opened: string;
}

export interface AppSettings {
  theme: string;
  recent_workspaces: RecentWorkspace[];
  default_embedding_provider: string;
  auto_update: boolean;
  openai_api_key: string;
}

export const currentView = writable<View>('welcome');
export const workspace = writable<WorkspaceInfo | null>(null);
export const recentWorkspaces = writable<RecentWorkspace[]>([]);
export const settings = writable<AppSettings | null>(null);
export const loading = writable(false);
export const error = writable<string | null>(null);
export const theme = writable<string>('system');

export const isWorkspaceOpen = derived(workspace, ($ws) => $ws !== null);

export function showError(msg: string) {
  error.set(msg);
  setTimeout(() => error.set(null), 5000);
}

export async function loadRecentWorkspaces() {
  try {
    const recent = await invoke<RecentWorkspace[]>('workspace_list_recent');
    recentWorkspaces.set(recent);
  } catch (e) {
    console.error('Failed to load recent workspaces:', e);
  }
}

export async function loadSettings() {
  try {
    const s = await invoke<AppSettings>('settings_get');
    settings.set(s);
    theme.set(s.theme);
    applyTheme(s.theme);
  } catch (e) {
    console.error('Failed to load settings:', e);
  }
}

export function applyTheme(t: string) {
  if (t === 'dark' || (t === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)) {
    document.documentElement.classList.add('dark');
  } else {
    document.documentElement.classList.remove('dark');
  }
}

export async function openWorkspace(path: string) {
  loading.set(true);
  error.set(null);
  try {
    const info = await invoke<WorkspaceInfo>('workspace_open', { path });
    workspace.set(info);
    currentView.set('dashboard');
    await loadRecentWorkspaces();
  } catch (e: any) {
    showError(typeof e === 'string' ? e : e.message || 'Failed to open workspace');
  } finally {
    loading.set(false);
  }
}

export async function createWorkspace(name: string, path: string) {
  loading.set(true);
  error.set(null);
  try {
    const info = await invoke<WorkspaceInfo>('workspace_create', { name, path });
    workspace.set(info);
    currentView.set('dashboard');
    await loadRecentWorkspaces();
  } catch (e: any) {
    showError(typeof e === 'string' ? e : e.message || 'Failed to create workspace');
  } finally {
    loading.set(false);
  }
}

export async function closeWorkspace() {
  try {
    await invoke('workspace_close');
    workspace.set(null);
    currentView.set('welcome');
  } catch (e: any) {
    showError(typeof e === 'string' ? e : e.message || 'Failed to close workspace');
  }
}

export async function refreshWorkspaceInfo() {
  try {
    const info = await invoke<WorkspaceInfo>('workspace_get_info');
    workspace.set(info);
  } catch (e) {
    console.error('Failed to refresh workspace info:', e);
  }
}
