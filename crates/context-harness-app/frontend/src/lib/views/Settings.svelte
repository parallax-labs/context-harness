<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { save } from '@tauri-apps/plugin-dialog';
  import { settings, theme, applyTheme, showError, type AppSettings } from '../stores/app';
  import { onMount } from 'svelte';

  let currentTheme = $state('system');
  let defaultProvider = $state('local');
  let autoUpdate = $state(true);
  let openaiKey = $state('');
  let saving = $state(false);

  type Tab = 'app' | 'workspace' | 'raw';
  let activeTab = $state<Tab>('app');

  let rawConfig = $state('');
  let configDirty = $state(false);
  let configSaving = $state(false);
  let configMessage = $state('');
  let configError = $state('');

  $effect(() => {
    if ($settings) {
      currentTheme = $settings.theme;
      defaultProvider = $settings.default_embedding_provider;
      autoUpdate = $settings.auto_update;
      openaiKey = $settings.openai_api_key ?? '';
    }
  });

  onMount(loadConfig);

  async function loadConfig() {
    try {
      const res = await invoke<{ raw: string }>('workspace_get_config');
      rawConfig = res.raw;
      configDirty = false;
      configError = '';
    } catch (_) {
      // no workspace open
    }
  }

  async function saveAppSettings() {
    saving = true;
    try {
      const updated: AppSettings = {
        theme: currentTheme,
        recent_workspaces: $settings?.recent_workspaces ?? [],
        default_embedding_provider: defaultProvider,
        auto_update: autoUpdate,
        openai_api_key: openaiKey,
      };
      await invoke('settings_update', { settings: updated });
      settings.set(updated);
      theme.set(currentTheme);
      applyTheme(currentTheme);
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to save settings');
    } finally {
      saving = false;
    }
  }

  async function saveConfig() {
    configSaving = true;
    configError = '';
    configMessage = '';
    try {
      await invoke('workspace_update_config', { raw: rawConfig });
      configDirty = false;
      configMessage = 'Configuration saved';
      setTimeout(() => { configMessage = ''; }, 3000);
    } catch (e: any) {
      configError = typeof e === 'string' ? e : 'Failed to save config';
    } finally {
      configSaving = false;
    }
  }

  async function exportConfig() {
    try {
      const filePath = await save({
        defaultPath: 'ctx.toml',
        filters: [{ name: 'TOML', extensions: ['toml'] }],
      });
      if (filePath) {
        await invoke('workspace_export_config', { destination: filePath });
        configMessage = `Exported to ${filePath}`;
        setTimeout(() => { configMessage = ''; }, 4000);
      }
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Export failed');
    }
  }

  async function copyConfig() {
    try {
      const res = await invoke<{ raw: string }>('workspace_get_config');
      await navigator.clipboard.writeText(res.raw);
      configMessage = 'Config copied to clipboard';
      setTimeout(() => { configMessage = ''; }, 3000);
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Copy failed');
    }
  }

  function handleConfigInput(e: Event) {
    rawConfig = (e.target as HTMLTextAreaElement).value;
    configDirty = true;
  }
</script>

<div class="p-6 h-full overflow-auto">
  <div class="flex items-center justify-between mb-6">
    <h2 class="text-2xl font-bold text-text">Settings</h2>
    <div class="flex gap-1 bg-surface-alt rounded-lg p-1">
      {#each [['app', 'Application'], ['workspace', 'Workspace Config'], ['raw', 'Raw TOML']] as [id, label]}
        <button
          class="px-4 py-1.5 rounded-md text-sm font-medium transition-colors
                 {activeTab === id ? 'bg-primary text-white' : 'text-text-muted hover:text-text'}"
          onclick={() => { activeTab = id as Tab; if (id !== 'app') loadConfig(); }}
        >{label}</button>
      {/each}
    </div>
  </div>

  {#if configMessage}
    <div class="mb-4 px-4 py-2.5 rounded-lg text-sm bg-success/10 text-success">
      {configMessage}
    </div>
  {/if}

  {#if configError}
    <div class="mb-4 px-4 py-2.5 rounded-lg text-sm bg-error/10 text-error">
      {configError}
    </div>
  {/if}

  {#if activeTab === 'app'}
    <div class="max-w-2xl space-y-6">
      <div class="bg-surface border border-border rounded-xl p-5">
        <h3 class="font-semibold text-text mb-4">Appearance</h3>
        <div>
          <label class="block text-sm text-text-muted mb-2">Theme</label>
          <div class="flex gap-3">
            {#each ['light', 'dark', 'system'] as opt}
              <button
                class="px-4 py-2 rounded-lg border text-sm font-medium transition-colors
                       {currentTheme === opt ? 'border-primary bg-primary/10 text-primary' : 'border-border text-text hover:bg-surface-alt'}"
                onclick={() => { currentTheme = opt; applyTheme(opt); }}
              >
                {opt.charAt(0).toUpperCase() + opt.slice(1)}
              </button>
            {/each}
          </div>
        </div>
      </div>

      <div class="bg-surface border border-border rounded-xl p-5">
        <h3 class="font-semibold text-text mb-4">Defaults</h3>
        <div>
          <label class="block text-sm text-text-muted mb-1">Default Embedding Provider</label>
          <select
            bind:value={defaultProvider}
            class="w-full px-3 py-2 border border-border rounded-md bg-surface text-text"
          >
            <option value="local">Local (fastembed)</option>
            <option value="openai">OpenAI</option>
            <option value="ollama">Ollama</option>
            <option value="none">None</option>
          </select>
        </div>
      </div>

      <div class="bg-surface border border-border rounded-xl p-5">
        <h3 class="font-semibold text-text mb-4">API Keys</h3>
        <div>
          <label for="openai-key" class="block text-sm text-text-muted mb-1">OpenAI API Key</label>
          <input
            id="openai-key"
            type="password"
            bind:value={openaiKey}
            placeholder="sk-..."
            class="w-full px-3 py-2 border border-border rounded-md bg-surface text-text font-mono text-sm"
          />
          <p class="text-xs text-text-muted mt-1">Required for OpenAI embeddings and search.</p>
        </div>
      </div>

      <div class="bg-surface border border-border rounded-xl p-5">
        <h3 class="font-semibold text-text mb-4">Updates</h3>
        <label class="flex items-center gap-3 cursor-pointer">
          <input
            type="checkbox"
            bind:checked={autoUpdate}
            class="w-4 h-4 rounded border-border text-primary focus:ring-primary"
          />
          <span class="text-sm text-text">Automatically check for updates</span>
        </label>
      </div>

      <button
        class="px-6 py-2.5 bg-primary text-white rounded-lg font-medium
               hover:bg-primary-hover transition-colors disabled:opacity-50"
        onclick={saveAppSettings}
        disabled={saving}
      >
        {saving ? 'Saving...' : 'Save Settings'}
      </button>
    </div>

  {:else if activeTab === 'workspace'}
    <div class="max-w-3xl space-y-4">
      <p class="text-sm text-text-muted">
        Edit your workspace's <code class="bg-surface-alt px-1 py-0.5 rounded text-xs">ctx.toml</code> configuration.
        Changes are validated before saving.
      </p>

      <div class="bg-surface border border-border rounded-xl p-5">
        <textarea
          class="w-full h-[60vh] font-mono text-sm bg-surface-alt border border-border rounded-lg p-4 text-text
                 focus:outline-none focus:ring-2 focus:ring-primary resize-none"
          value={rawConfig}
          oninput={handleConfigInput}
          spellcheck={false}
        ></textarea>
      </div>

      <div class="flex items-center gap-3">
        <button
          class="px-5 py-2.5 bg-primary text-white rounded-lg font-medium
                 hover:bg-primary-hover transition-colors disabled:opacity-50"
          onclick={saveConfig}
          disabled={configSaving || !configDirty}
        >
          {configSaving ? 'Saving...' : 'Save Config'}
        </button>
        <button
          class="px-5 py-2.5 border border-border text-text rounded-lg font-medium
                 hover:bg-surface-alt transition-colors"
          onclick={exportConfig}
        >
          Export to File
        </button>
        <button
          class="px-5 py-2.5 border border-border text-text rounded-lg font-medium
                 hover:bg-surface-alt transition-colors"
          onclick={copyConfig}
        >
          Copy to Clipboard
        </button>
        {#if configDirty}
          <span class="text-xs text-warning">Unsaved changes</span>
        {/if}
      </div>
    </div>

  {:else if activeTab === 'raw'}
    <div class="max-w-3xl space-y-4">
      <p class="text-sm text-text-muted">
        Read-only view of the resolved configuration. Edit in the "Workspace Config" tab.
      </p>

      <div class="bg-surface border border-border rounded-xl p-5">
        <pre class="font-mono text-sm text-text whitespace-pre-wrap overflow-auto max-h-[65vh]">{rawConfig}</pre>
      </div>

      <div class="flex gap-3">
        <button
          class="px-5 py-2.5 border border-border text-text rounded-lg font-medium
                 hover:bg-surface-alt transition-colors"
          onclick={exportConfig}
        >
          Export to File
        </button>
        <button
          class="px-5 py-2.5 border border-border text-text rounded-lg font-medium
                 hover:bg-surface-alt transition-colors"
          onclick={copyConfig}
        >
          Copy to Clipboard
        </button>
      </div>
    </div>
  {/if}
</div>
