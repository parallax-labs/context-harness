<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { showError } from '../stores/app';
  import { onMount } from 'svelte';

  interface RegistryInfo {
    name: string;
    path: string;
    url: string | null;
    readonly: boolean;
    is_git: boolean;
    installed: boolean;
    connectors: number;
    tools: number;
    agents: number;
  }

  interface RegistryExtension {
    name: string;
    description: string;
    extension_type: string;
    registry: string;
    tags: string[];
    required_config: string[];
  }

  let registries = $state<RegistryInfo[]>([]);
  let extensions = $state<RegistryExtension[]>([]);
  let searchQuery = $state('');
  let typeFilter = $state('');
  let initializing = $state(false);
  let updating = $state(false);
  let updateMessage = $state('');

  let initUrl = $state('https://github.com/parallax-labs/ctx-registry.git');
  let initName = $state('community');
  let showInitForm = $state(false);
  let installing = $state<string | null>(null);

  onMount(async () => {
    await loadAll();
  });

  async function loadAll() {
    await Promise.all([loadRegistries(), loadExtensions()]);
  }

  async function loadRegistries() {
    try {
      registries = await invoke<RegistryInfo[]>('registry_status');
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to load registry status');
    }
  }

  async function loadExtensions() {
    try {
      extensions = await invoke<RegistryExtension[]>('registry_list_extensions', {
        extensionType: typeFilter || null,
      });
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to load extensions');
    }
  }

  async function searchExtensions() {
    if (!searchQuery.trim()) {
      await loadExtensions();
      return;
    }
    try {
      extensions = await invoke<RegistryExtension[]>('registry_search', {
        query: searchQuery.trim(),
      });
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Search failed');
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') searchExtensions();
  }

  async function initRegistry() {
    initializing = true;
    try {
      const info = await invoke<RegistryInfo>('registry_init', {
        url: initUrl || null,
        name: initName || null,
      });
      showInitForm = false;
      updateMessage = `Initialized "${info.name}": ${info.connectors} connectors, ${info.tools} tools, ${info.agents} agents`;
      setTimeout(() => updateMessage = '', 6000);
      await loadAll();
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to initialize registry');
    } finally {
      initializing = false;
    }
  }

  async function updateRegistries(name?: string) {
    updating = true;
    try {
      const msg = await invoke<string>('registry_update', {
        registryName: name || null,
      });
      updateMessage = msg;
      setTimeout(() => updateMessage = '', 6000);
      await loadAll();
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to update registries');
    } finally {
      updating = false;
    }
  }

  async function installRegistry(name: string) {
    updating = true;
    try {
      const msg = await invoke<string>('registry_install', {
        registryName: name,
      });
      updateMessage = msg;
      setTimeout(() => updateMessage = '', 6000);
      await loadAll();
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to install registry');
    } finally {
      updating = false;
    }
  }

  async function addExtension(ext: RegistryExtension) {
    installing = ext.name;
    try {
      const msg = await invoke<string>('registry_add_extension', {
        extensionType: ext.extension_type,
        extensionName: ext.name,
      });
      updateMessage = msg;
      setTimeout(() => updateMessage = '', 8000);
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to add extension');
    } finally {
      installing = null;
    }
  }

  $effect(() => {
    typeFilter;
    loadExtensions();
  });

  const grouped = $derived(
    extensions.reduce((acc, ext) => {
      if (!acc[ext.extension_type]) acc[ext.extension_type] = [];
      acc[ext.extension_type].push(ext);
      return acc;
    }, {} as Record<string, RegistryExtension[]>)
  );

  const hasRegistries = $derived(registries.length > 0);
  const hasInstalledRegistries = $derived(registries.some(r => r.installed));
  const hasGitRegistries = $derived(registries.some(r => r.is_git && r.installed));
</script>

<div class="p-6">
  <div class="flex items-center justify-between mb-6">
    <h2 class="text-2xl font-bold text-text">Registry</h2>
    <div class="flex gap-2">
      {#if hasGitRegistries}
        <button
          class="px-3 py-1.5 text-sm border border-border rounded-lg text-text
                 hover:bg-surface-alt transition-colors disabled:opacity-50"
          onclick={() => updateRegistries()}
          disabled={updating}
        >
          {updating ? 'Updating...' : 'Update All'}
        </button>
      {/if}
      <button
        class="px-3 py-1.5 text-sm bg-primary text-white rounded-lg font-medium
               hover:bg-primary-hover transition-colors disabled:opacity-50"
        onclick={() => showInitForm = !showInitForm}
        disabled={initializing}
      >
        Add Registry
      </button>
    </div>
  </div>

  {#if updateMessage}
    <div class="mb-4 px-4 py-3 rounded-lg text-sm bg-green-50 text-green-700 dark:bg-green-900/20 dark:text-green-300">
      {updateMessage}
    </div>
  {/if}

  {#if showInitForm}
    <div class="mb-6 bg-surface border border-border rounded-xl p-5">
      <h3 class="font-semibold text-text mb-3">Add Extension Registry</h3>
      <div class="grid grid-cols-2 gap-4 mb-4">
        <div>
          <label for="reg-name" class="block text-sm text-text-muted mb-1">Name</label>
          <input
            id="reg-name"
            type="text"
            bind:value={initName}
            placeholder="community"
            class="w-full px-3 py-2 border border-border rounded-md bg-surface text-text
                   placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-primary"
          />
        </div>
        <div>
          <label for="reg-url" class="block text-sm text-text-muted mb-1">Git URL</label>
          <input
            id="reg-url"
            type="text"
            bind:value={initUrl}
            placeholder="https://github.com/org/registry.git"
            class="w-full px-3 py-2 border border-border rounded-md bg-surface text-text
                   placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-primary"
          />
        </div>
      </div>
      <div class="flex gap-2">
        <button
          class="px-4 py-2 bg-primary text-white rounded-lg font-medium
                 hover:bg-primary-hover transition-colors disabled:opacity-50"
          onclick={initRegistry}
          disabled={initializing}
        >
          {initializing ? 'Cloning...' : 'Clone & Initialize'}
        </button>
        <button
          class="px-4 py-2 border border-border rounded-lg text-text
                 hover:bg-surface-alt transition-colors"
          onclick={() => showInitForm = false}
        >
          Cancel
        </button>
      </div>
    </div>
  {/if}

  {#if registries.length > 0}
    <div class="mb-6">
      <h3 class="text-sm font-medium text-text-muted uppercase tracking-wider mb-3">Configured Registries</h3>
      <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
        {#each registries as reg}
          <div class="bg-surface border border-border rounded-xl p-4">
            <div class="flex items-start justify-between">
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <span class="font-medium text-text">{reg.name}</span>
                  {#if reg.installed}
                    <span class="px-1.5 py-0.5 text-[10px] bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 rounded">installed</span>
                  {:else}
                    <span class="px-1.5 py-0.5 text-[10px] bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400 rounded">not installed</span>
                  {/if}
                  {#if reg.is_git}
                    <span class="px-1.5 py-0.5 text-[10px] bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400 rounded">git</span>
                  {/if}
                  {#if reg.readonly}
                    <span class="px-1.5 py-0.5 text-[10px] bg-surface-alt text-text-muted rounded">readonly</span>
                  {/if}
                </div>
                <div class="text-xs text-text-muted mt-1 truncate">{reg.path}</div>
                {#if reg.url}
                  <div class="text-xs text-text-muted truncate">{reg.url}</div>
                {/if}
                {#if reg.installed}
                  <div class="text-xs text-text-muted mt-2">
                    {reg.connectors} connectors, {reg.tools} tools, {reg.agents} agents
                  </div>
                {/if}
              </div>
              <div class="flex gap-1 ml-2">
                {#if !reg.installed && reg.url}
                  <button
                    class="px-2 py-1 text-xs bg-primary text-white rounded
                           hover:bg-primary-hover transition-colors disabled:opacity-50"
                    onclick={() => installRegistry(reg.name)}
                    disabled={updating}
                  >
                    Install
                  </button>
                {/if}
                {#if reg.is_git && reg.installed}
                  <button
                    class="px-2 py-1 text-xs border border-border rounded text-text
                           hover:bg-surface-alt transition-colors disabled:opacity-50"
                    onclick={() => updateRegistries(reg.name)}
                    disabled={updating}
                  >
                    Pull
                  </button>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  {#if !hasRegistries && !showInitForm}
    <div class="text-center py-12">
      <div class="text-text-muted mb-4">No extension registries configured.</div>
      <button
        class="px-4 py-2 bg-primary text-white rounded-lg font-medium
               hover:bg-primary-hover transition-colors"
        onclick={() => showInitForm = true}
      >
        Initialize Community Registry
      </button>
      <p class="text-xs text-text-muted mt-3">
        The community registry provides connectors, tools, and agents you can add to your workspace.
      </p>
    </div>
  {:else if hasInstalledRegistries}
    <div class="flex gap-3 mb-6">
      <input
        type="text"
        placeholder="Search extensions..."
        bind:value={searchQuery}
        onkeydown={handleKeydown}
        class="flex-1 px-4 py-2.5 border border-border rounded-lg bg-surface text-text
               placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-primary"
      />
      <select
        bind:value={typeFilter}
        class="px-3 py-2.5 border border-border rounded-lg bg-surface text-text"
      >
        <option value="">All Types</option>
        <option value="tool">Tools</option>
        <option value="agent">Agents</option>
        <option value="connector">Connectors</option>
      </select>
    </div>

    {#if extensions.length === 0}
      <div class="text-center py-8 text-text-muted">
        No extensions found matching your filter.
      </div>
    {:else}
      {#each Object.entries(grouped) as [type, exts]}
        <div class="mb-6">
          <h3 class="text-sm font-medium text-text-muted uppercase tracking-wider mb-3">
            {type}s
            <span class="text-xs font-normal ml-1">({exts.length})</span>
          </h3>
          <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
            {#each exts as ext}
              <div class="bg-surface border border-border rounded-xl p-4 hover:border-primary/30 transition-colors">
                <div class="flex items-start justify-between gap-3">
                  <div class="flex-1 min-w-0">
                    <div class="font-medium text-text">{ext.name}</div>
                    <div class="text-sm text-text-muted mt-1">{ext.description}</div>
                    {#if ext.tags.length > 0}
                      <div class="flex flex-wrap gap-1 mt-2">
                        {#each ext.tags as tag}
                          <span class="px-1.5 py-0.5 text-[10px] bg-surface-alt border border-border rounded text-text-muted">
                            {tag}
                          </span>
                        {/each}
                      </div>
                    {/if}
                    {#if ext.required_config.length > 0}
                      <div class="text-xs text-text-muted mt-2">
                        Requires: {ext.required_config.join(', ')}
                      </div>
                    {/if}
                    <div class="text-xs text-text-muted mt-1">
                      from <span class="font-medium">{ext.registry}</span>
                    </div>
                  </div>
                  <button
                    class="flex-shrink-0 px-3 py-1.5 text-xs bg-primary text-white rounded-lg
                           hover:bg-primary-hover transition-colors disabled:opacity-50"
                    onclick={() => addExtension(ext)}
                    disabled={installing === ext.name}
                  >
                    {installing === ext.name ? 'Adding...' : 'Add to Workspace'}
                  </button>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/each}
    {/if}
  {/if}
</div>
