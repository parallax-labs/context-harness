<script lang="ts">
  import { onMount } from 'svelte';
  import { currentView, isWorkspaceOpen, loadRecentWorkspaces, loadSettings } from './lib/stores/app';
  import Sidebar from './lib/components/Sidebar.svelte';
  import Toast from './lib/components/Toast.svelte';
  import Welcome from './lib/views/Welcome.svelte';
  import Dashboard from './lib/views/Dashboard.svelte';
  import Search from './lib/views/Search.svelte';
  import Documents from './lib/views/Documents.svelte';
  import Connectors from './lib/views/Connectors.svelte';
  import Embeddings from './lib/views/Embeddings.svelte';
  import Agents from './lib/views/Agents.svelte';
  import Registry from './lib/views/Registry.svelte';
  import Settings from './lib/views/Settings.svelte';

  onMount(async () => {
    await loadSettings();
    await loadRecentWorkspaces();

    window.addEventListener('keydown', (e) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        if ($isWorkspaceOpen) {
          currentView.set('search');
        }
      }
    });
  });
</script>

<div class="h-full flex bg-surface-alt text-text">
  {#if $isWorkspaceOpen}
    <Sidebar />
    <main class="flex-1 overflow-auto">
      {#if $currentView === 'dashboard'}
        <Dashboard />
      {:else if $currentView === 'search'}
        <Search />
      {:else if $currentView === 'documents'}
        <Documents />
      {:else if $currentView === 'connectors'}
        <Connectors />
      {:else if $currentView === 'embeddings'}
        <Embeddings />
      {:else if $currentView === 'agents'}
        <Agents />
      {:else if $currentView === 'registry'}
        <Registry />
      {:else if $currentView === 'settings'}
        <Settings />
      {/if}
    </main>
  {:else}
    <Welcome />
  {/if}
</div>

<Toast />
