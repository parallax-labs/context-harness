<script lang="ts">
  import { recentWorkspaces, openWorkspace, createWorkspace, loading } from '../stores/app';
  import { open } from '@tauri-apps/plugin-dialog';

  let newName = $state('');
  let showCreate = $state(false);

  async function handleOpen() {
    const selected = await open({ directory: true, title: 'Open Workspace' });
    if (selected) {
      await openWorkspace(selected as string);
    }
  }

  async function handleCreate() {
    const selected = await open({ directory: true, title: 'Choose Location' });
    if (selected && newName.trim()) {
      const path = `${selected}/${newName.trim()}`;
      await createWorkspace(newName.trim(), path);
    }
  }
</script>

<div class="h-full flex items-center justify-center bg-surface">
  <div class="max-w-lg w-full px-8">
    <div class="text-center mb-10">
      <h1 class="text-4xl font-bold text-text tracking-tight">Context Harness</h1>
      <p class="text-text-muted mt-2">Your knowledge base, on your device.</p>
    </div>

    <div class="space-y-3">
      <button
        class="w-full py-3 px-4 bg-primary text-white rounded-lg font-medium
               hover:bg-primary-hover transition-colors disabled:opacity-50"
        onclick={handleOpen}
        disabled={$loading}
      >
        Open Workspace
      </button>

      {#if !showCreate}
        <button
          class="w-full py-3 px-4 border border-border rounded-lg text-text
                 hover:bg-surface-alt transition-colors"
          onclick={() => showCreate = true}
        >
          Create New Workspace
        </button>
      {:else}
        <div class="border border-border rounded-lg p-4 space-y-3">
          <input
            type="text"
            placeholder="Workspace name"
            bind:value={newName}
            class="w-full px-3 py-2 border border-border rounded-md bg-surface text-text
                   placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-primary"
          />
          <div class="flex gap-2">
            <button
              class="flex-1 py-2 px-4 bg-primary text-white rounded-md font-medium
                     hover:bg-primary-hover transition-colors disabled:opacity-50"
              onclick={handleCreate}
              disabled={!newName.trim() || $loading}
            >
              Create
            </button>
            <button
              class="py-2 px-4 border border-border rounded-md text-text-muted
                     hover:bg-surface-alt transition-colors"
              onclick={() => { showCreate = false; newName = ''; }}
            >
              Cancel
            </button>
          </div>
        </div>
      {/if}
    </div>

    {#if $recentWorkspaces.length > 0}
      <div class="mt-8">
        <h2 class="text-sm font-medium text-text-muted uppercase tracking-wider mb-3">
          Recent Workspaces
        </h2>
        <div class="space-y-1">
          {#each $recentWorkspaces as recent}
            <button
              class="w-full text-left px-4 py-3 rounded-lg hover:bg-surface-alt
                     transition-colors group"
              onclick={() => openWorkspace(recent.path)}
            >
              <div class="text-sm font-medium text-text">{recent.name}</div>
              <div class="text-xs text-text-muted truncate">{recent.path}</div>
            </button>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>
