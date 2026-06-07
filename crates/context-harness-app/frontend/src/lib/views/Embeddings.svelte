<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { showError } from '../stores/app';
  import { onMount, onDestroy } from 'svelte';

  interface EmbeddingStatus {
    provider: string;
    model: string | null;
    total_chunks: number;
    embedded_chunks: number;
    pending_chunks: number;
    stale_chunks: number;
    model_status: string;
    model_error: string | null;
  }

  let status = $state<EmbeddingStatus | null>(null);
  let working = $state(false);
  let workMessage = $state('');
  let provider = $state('');
  let model = $state('');
  let dims = $state('');
  let editingConfig = $state(false);

  let unlisten: (() => void) | null = null;

  onMount(async () => {
    await loadStatus();
    unlisten = await listen<any>('embed-progress', (event) => {
      const p = event.payload;
      if (p.phase === 'complete') {
        working = false;
        workMessage = p.message;
        loadStatus();
        setTimeout(() => workMessage = '', 6000);
      } else if (p.phase === 'error') {
        working = false;
        workMessage = p.message;
        loadStatus();
      } else if (p.phase === 'initializing') {
        working = true;
        workMessage = p.message || 'Initializing embedding provider...';
      } else {
        working = true;
        workMessage = p.message || 'Processing...';
      }
    });
  });

  onDestroy(() => { if (unlisten) unlisten(); });

  async function loadStatus() {
    try {
      status = await invoke<EmbeddingStatus>('embedding_status');
      provider = status.provider;
      model = status.model ?? '';
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to load embedding status');
    }
  }

  async function embedPending() {
    working = true;
    workMessage = 'Embedding pending chunks...';
    try {
      await invoke('embedding_run_pending');
    } catch (e: any) {
      working = false;
      showError(typeof e === 'string' ? e : 'Failed to start embedding');
    }
  }

  async function rebuild() {
    working = true;
    workMessage = 'Rebuilding all embeddings...';
    try {
      await invoke('embedding_rebuild');
    } catch (e: any) {
      working = false;
      showError(typeof e === 'string' ? e : 'Failed to start rebuild');
    }
  }

  async function saveConfig() {
    try {
      await invoke('embedding_update_config', {
        provider,
        model: model || null,
        dims: dims ? parseInt(dims) : null,
      });
      editingConfig = false;
      await loadStatus();
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to update embedding config');
    }
  }

  function coveragePercent(s: EmbeddingStatus): number {
    if (s.total_chunks === 0) return 0;
    return Math.round((s.embedded_chunks / s.total_chunks) * 100);
  }
</script>

<div class="p-6">
  <h2 class="text-2xl font-bold text-text mb-6">Embeddings</h2>

  {#if workMessage}
    <div class="mb-4 px-4 py-3 rounded-lg text-sm
                {working ? 'bg-blue-50 text-blue-700 dark:bg-blue-900/20 dark:text-blue-300' : 'bg-green-50 text-green-700 dark:bg-green-900/20 dark:text-green-300'}">
      {workMessage}
    </div>
  {/if}

  {#if status}
    {#if status.model_error}
      <div class="mb-4 px-4 py-3 rounded-lg bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800">
        <div class="text-sm font-medium text-red-700 dark:text-red-300">Provider Error</div>
        <div class="text-sm text-red-600 dark:text-red-400 mt-1 font-mono">{status.model_error}</div>
      </div>
    {/if}

    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 mb-6">
      <div class="bg-surface border border-border rounded-xl p-5">
        <div class="text-sm text-text-muted">Provider</div>
        <div class="text-xl font-semibold text-text mt-1">{status.provider}</div>
        {#if status.model}
          <div class="text-xs text-text-muted mt-1">{status.model}</div>
        {/if}
        <div class="text-xs mt-2 {status.model_status.startsWith('ready') ? 'text-green-600 dark:text-green-400' : status.model_status === 'disabled' ? 'text-text-muted' : 'text-red-500'}">
          {status.model_status}
        </div>
      </div>
      <div class="bg-surface border border-border rounded-xl p-5">
        <div class="text-sm text-text-muted">Coverage</div>
        <div class="text-3xl font-bold text-text mt-1">{coveragePercent(status)}%</div>
        <div class="text-xs text-text-muted mt-1">
          {status.embedded_chunks} / {status.total_chunks} chunks
        </div>
        {#if status.total_chunks > 0 && status.embedded_chunks === 0}
          <div class="text-xs text-warning mt-1">No embeddings yet — click "Embed Pending" to generate</div>
        {/if}
      </div>
      <div class="bg-surface border border-border rounded-xl p-5">
        <div class="text-sm text-text-muted">Pending</div>
        <div class="text-3xl font-bold text-warning mt-1">{status.pending_chunks}</div>
      </div>
      <div class="bg-surface border border-border rounded-xl p-5">
        <div class="text-sm text-text-muted">Stale</div>
        <div class="text-3xl font-bold text-error mt-1">{status.stale_chunks}</div>
      </div>
    </div>

    {#if status.total_chunks > 0}
      <div class="mb-6">
        <div class="w-full bg-surface-alt rounded-full h-3 overflow-hidden">
          <div
            class="bg-primary h-full rounded-full transition-all duration-500"
            style="width: {coveragePercent(status)}%"
          ></div>
        </div>
      </div>
    {/if}

    <div class="flex gap-3 mb-8">
      <button
        class="px-4 py-2 bg-primary text-white rounded-lg font-medium
               hover:bg-primary-hover transition-colors disabled:opacity-50"
        onclick={embedPending}
        disabled={working || status.pending_chunks === 0}
      >
        Embed Pending ({status.pending_chunks})
      </button>
      <button
        class="px-4 py-2 border border-border rounded-lg text-text
               hover:bg-surface-alt transition-colors disabled:opacity-50"
        onclick={rebuild}
        disabled={working}
      >
        Rebuild All
      </button>
      <button
        class="px-4 py-2 border border-border rounded-lg text-text
               hover:bg-surface-alt transition-colors"
        onclick={() => editingConfig = !editingConfig}
      >
        {editingConfig ? 'Cancel' : 'Configure'}
      </button>
    </div>

    {#if editingConfig}
      <div class="bg-surface border border-border rounded-xl p-5 max-w-lg">
        <h3 class="font-semibold text-text mb-4">Embedding Configuration</h3>
        <div class="space-y-3">
          <div>
            <label class="block text-sm text-text-muted mb-1">Provider</label>
            <select
              bind:value={provider}
              class="w-full px-3 py-2 border border-border rounded-md bg-surface text-text"
            >
              <option value="local">Local (fastembed)</option>
              <option value="openai">OpenAI</option>
              <option value="ollama">Ollama</option>
              <option value="none">Disabled</option>
            </select>
          </div>
          <div>
            <label class="block text-sm text-text-muted mb-1">Model</label>
            <input
              type="text"
              bind:value={model}
              placeholder="e.g. text-embedding-3-small"
              class="w-full px-3 py-2 border border-border rounded-md bg-surface text-text
                     placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-primary"
            />
          </div>
          <div>
            <label class="block text-sm text-text-muted mb-1">Dimensions</label>
            <input
              type="text"
              bind:value={dims}
              placeholder="e.g. 384"
              class="w-full px-3 py-2 border border-border rounded-md bg-surface text-text
                     placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-primary"
            />
          </div>
          <button
            class="px-4 py-2 bg-primary text-white rounded-md font-medium
                   hover:bg-primary-hover transition-colors"
            onclick={saveConfig}
          >
            Save Configuration
          </button>
        </div>
      </div>
    {/if}
  {:else}
    <div class="text-center py-12 text-text-muted">Loading...</div>
  {/if}
</div>
