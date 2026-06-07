<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { showError } from '../stores/app';
  import { onMount } from 'svelte';

  interface ConnectorInfo {
    name: string;
    connector_type: string;
    document_count: number;
    last_sync: string | null;
    healthy: boolean;
    notes: string | null;
  }

  interface ConnectorTestResult {
    success: boolean;
    message: string;
  }

  let connectors = $state<ConnectorInfo[]>([]);
  let showAddForm = $state(false);
  let newType = $state('filesystem');
  let newName = $state('');
  let newConfig = $state<Record<string, string>>({ root: '' });
  let testResult = $state<ConnectorTestResult | null>(null);

  onMount(loadConnectors);

  async function loadConnectors() {
    try {
      connectors = await invoke<ConnectorInfo[]>('connector_list');
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to load connectors');
    }
  }

  async function addConnector() {
    if (!newName.trim()) return;
    try {
      await invoke('connector_add', {
        connectorType: newType,
        name: newName.trim(),
        config: newConfig,
      });
      showAddForm = false;
      newName = '';
      newConfig = { root: '' };
      await loadConnectors();
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to add connector');
    }
  }

  async function removeConnector(type: string, name: string) {
    try {
      await invoke('connector_remove', {
        connectorType: type,
        name,
        purgeDocuments: false,
      });
      await loadConnectors();
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to remove connector');
    }
  }

  async function testConnector(type: string, name: string) {
    try {
      testResult = await invoke<ConnectorTestResult>('connector_test', {
        connectorType: type,
        name,
      });
      setTimeout(() => testResult = null, 4000);
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to test connector');
    }
  }

  async function syncConnector(name: string) {
    try {
      await invoke('sync_start', { target: name });
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to start sync');
    }
  }

  function updateNewType(type: string) {
    newType = type;
    if (type === 'filesystem') {
      newConfig = { root: '' };
    } else if (type === 'git') {
      newConfig = { url: '', branch: 'main', root: '/' };
    } else if (type === 's3') {
      newConfig = { bucket: '', prefix: '', region: 'us-east-1' };
    }
  }
</script>

<div class="p-6">
  <div class="flex items-center justify-between mb-6">
    <h2 class="text-2xl font-bold text-text">Connectors</h2>
    <button
      class="px-4 py-2 bg-primary text-white rounded-lg font-medium
             hover:bg-primary-hover transition-colors"
      onclick={() => showAddForm = !showAddForm}
    >
      {showAddForm ? 'Cancel' : 'Add Connector'}
    </button>
  </div>

  {#if testResult}
    <div class="mb-4 px-4 py-3 rounded-lg text-sm
                {testResult.success ? 'bg-green-50 text-green-700 dark:bg-green-900/20 dark:text-green-300' : 'bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300'}">
      {testResult.message}
    </div>
  {/if}

  {#if showAddForm}
    <div class="bg-surface border border-border rounded-xl p-5 mb-6">
      <h3 class="font-semibold text-text mb-4">Add Connector</h3>
      <div class="grid grid-cols-2 gap-4 mb-4">
        <div>
          <label class="block text-sm text-text-muted mb-1">Type</label>
          <select
            value={newType}
            onchange={(e) => updateNewType((e.target as HTMLSelectElement).value)}
            class="w-full px-3 py-2 border border-border rounded-md bg-surface text-text"
          >
            <option value="filesystem">Filesystem</option>
            <option value="git">Git</option>
            <option value="s3">S3</option>
          </select>
        </div>
        <div>
          <label class="block text-sm text-text-muted mb-1">Name</label>
          <input
            type="text"
            placeholder="my-source"
            bind:value={newName}
            class="w-full px-3 py-2 border border-border rounded-md bg-surface text-text
                   placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-primary"
          />
        </div>
      </div>
      {#each Object.entries(newConfig) as [key, val]}
        <div class="mb-3">
          <label class="block text-sm text-text-muted mb-1">{key}</label>
          <input
            type="text"
            value={val}
            oninput={(e) => newConfig[key] = (e.target as HTMLInputElement).value}
            class="w-full px-3 py-2 border border-border rounded-md bg-surface text-text
                   focus:outline-none focus:ring-2 focus:ring-primary"
          />
        </div>
      {/each}
      <button
        class="px-4 py-2 bg-primary text-white rounded-md font-medium
               hover:bg-primary-hover transition-colors disabled:opacity-50"
        onclick={addConnector}
        disabled={!newName.trim()}
      >
        Add
      </button>
    </div>
  {/if}

  {#if connectors.length === 0}
    <div class="text-center py-12 text-text-muted">
      No connectors configured. Add one to start ingesting documents.
    </div>
  {:else}
    <div class="space-y-3">
      {#each connectors as conn}
        <div class="bg-surface border border-border rounded-xl p-5">
          <div class="flex items-start justify-between">
            <div>
              <div class="flex items-center gap-2">
                <span class="w-2 h-2 rounded-full {conn.healthy ? 'bg-success' : 'bg-error'}"></span>
                <span class="font-medium text-text">{conn.name}</span>
                <span class="text-xs px-2 py-0.5 bg-surface-alt text-text-muted rounded-full">
                  {conn.connector_type}
                </span>
              </div>
              <div class="text-sm text-text-muted mt-1">
                {conn.document_count} documents
                {#if conn.last_sync}
                  · Last synced: {conn.last_sync}
                {/if}
              </div>
              {#if conn.notes}
                <div class="text-xs text-text-muted mt-1">{conn.notes}</div>
              {/if}
            </div>
            <div class="flex gap-2">
              <button
                class="px-3 py-1.5 text-sm border border-border rounded-md
                       hover:bg-surface-alt transition-colors text-text"
                onclick={() => testConnector(conn.connector_type, conn.name.split(':')[1] || conn.name)}
              >Test</button>
              <button
                class="px-3 py-1.5 text-sm bg-primary/10 text-primary rounded-md
                       hover:bg-primary/20 transition-colors"
                onclick={() => syncConnector(conn.name)}
              >Sync</button>
              <button
                class="px-3 py-1.5 text-sm text-error border border-error/20 rounded-md
                       hover:bg-error/10 transition-colors"
                onclick={() => removeConnector(conn.connector_type, conn.name.split(':')[1] || conn.name)}
              >Remove</button>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>
