<script lang="ts">
  import { workspace, refreshWorkspaceInfo } from '../stores/app';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, onDestroy } from 'svelte';

  let syncing = $state(false);
  let syncMessage = $state('');
  let syncPhase = $state('');
  let syncCurrent = $state(0);
  let syncTotal = $state<number | null>(null);
  let syncElapsed = $state(0);

  let unlisten: (() => void) | null = null;
  let refreshInterval: ReturnType<typeof setInterval> | null = null;

  onMount(async () => {
    refreshWorkspaceInfo();

    unlisten = await listen<any>('sync-progress', (event) => {
      const p = event.payload;
      syncPhase = p.phase;
      syncElapsed = p.elapsed_ms || 0;

      if (p.phase === 'complete' || p.phase === 'error') {
        syncing = false;
        syncMessage = p.message || '';
        stopPolling();
        refreshWorkspaceInfo();
        setTimeout(() => { syncMessage = ''; syncPhase = ''; }, 5000);
      } else {
        syncing = true;
        syncCurrent = p.current || 0;
        syncTotal = p.total ?? null;
        syncMessage = p.message || 'Syncing...';
        startPolling();
      }
    });
  });

  onDestroy(() => {
    if (unlisten) unlisten();
    stopPolling();
  });

  function startPolling() {
    if (refreshInterval) return;
    refreshInterval = setInterval(() => {
      refreshWorkspaceInfo();
    }, 3000);
  }

  function stopPolling() {
    if (refreshInterval) {
      clearInterval(refreshInterval);
      refreshInterval = null;
    }
  }

  async function syncAll() {
    syncing = true;
    syncMessage = 'Starting sync...';
    syncPhase = 'scanning';
    syncCurrent = 0;
    syncTotal = null;
    try {
      await invoke('sync_start', { target: 'all' });
    } catch (e: any) {
      syncing = false;
      syncMessage = `Failed: ${e}`;
      syncPhase = 'error';
    }
  }

  function formatNumber(n: number): string {
    return n.toLocaleString();
  }

  function formatElapsed(ms: number): string {
    if (ms < 1000) return `${ms}ms`;
    const s = ms / 1000;
    if (s < 60) return `${s.toFixed(1)}s`;
    const m = Math.floor(s / 60);
    const rem = Math.floor(s % 60);
    return `${m}m ${rem}s`;
  }
</script>

<div class="p-6 space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-2xl font-bold text-text">{$workspace?.name ?? 'Dashboard'}</h2>
      <p class="text-sm text-text-muted mt-1">{$workspace?.path}</p>
    </div>
    <button
      class="px-4 py-2 bg-primary text-white rounded-lg font-medium
             hover:bg-primary-hover transition-colors disabled:opacity-50"
      onclick={syncAll}
      disabled={syncing}
    >
      {syncing ? 'Syncing...' : 'Sync All'}
    </button>
  </div>

  {#if syncing}
    <div class="bg-surface border border-border rounded-xl p-5">
      <div class="flex items-center justify-between mb-2">
        <div class="flex items-center gap-2">
          <div class="w-2 h-2 rounded-full bg-primary animate-pulse"></div>
          <span class="text-sm font-medium text-text">
            {syncPhase === 'scanning' ? 'Scanning...'
            : syncPhase === 'discovering' ? 'Discovering files...'
            : syncPhase === 'ingesting' ? 'Ingesting documents...'
            : 'Syncing...'}
          </span>
        </div>
        <span class="text-xs text-text-muted">{formatElapsed(syncElapsed)}</span>
      </div>

      {#if syncTotal && syncTotal > 0}
        <div class="w-full bg-surface-alt rounded-full h-2 mb-2">
          <div
            class="bg-primary rounded-full h-2 transition-all duration-300"
            style="width: {Math.min(100, (syncCurrent / syncTotal) * 100)}%"
          ></div>
        </div>
        <div class="text-xs text-text-muted">
          {formatNumber(syncCurrent)} / {formatNumber(syncTotal)} documents
        </div>
      {/if}

      <div class="text-sm text-text-muted mt-1">{syncMessage}</div>
    </div>
  {:else if syncMessage}
    <div class="px-4 py-3 rounded-lg text-sm
                {syncPhase === 'error'
                  ? 'bg-error/10 text-error'
                  : 'bg-success/10 text-success'}">
      {syncMessage}
    </div>
  {/if}

  <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
    <div class="bg-surface border border-border rounded-xl p-5">
      <div class="text-sm text-text-muted">Documents</div>
      <div class="text-3xl font-bold text-text mt-1">
        {formatNumber($workspace?.document_count ?? 0)}
      </div>
    </div>
    <div class="bg-surface border border-border rounded-xl p-5">
      <div class="text-sm text-text-muted">Chunks</div>
      <div class="text-3xl font-bold text-text mt-1">
        {formatNumber($workspace?.chunk_count ?? 0)}
      </div>
    </div>
    <div class="bg-surface border border-border rounded-xl p-5">
      <div class="text-sm text-text-muted">Embedded</div>
      <div class="text-3xl font-bold text-text mt-1">
        {formatNumber($workspace?.embedded_chunk_count ?? 0)}
      </div>
    </div>
    <div class="bg-surface border border-border rounded-xl p-5">
      <div class="text-sm text-text-muted">Last Sync</div>
      <div class="text-xl font-semibold text-text mt-1">
        {$workspace?.last_sync ?? 'Never'}
      </div>
    </div>
  </div>
</div>
