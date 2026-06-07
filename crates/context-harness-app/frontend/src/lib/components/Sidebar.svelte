<script lang="ts">
  import { currentView, workspace, closeWorkspace, type View } from '../stores/app';

  interface NavItem {
    id: View;
    label: string;
    icon: string;
  }

  const navItems: NavItem[] = [
    { id: 'dashboard', label: 'Dashboard', icon: '◉' },
    { id: 'search', label: 'Search', icon: '⌕' },
    { id: 'documents', label: 'Documents', icon: '◫' },
    { id: 'connectors', label: 'Connectors', icon: '⇄' },
    { id: 'embeddings', label: 'Embeddings', icon: '◈' },
    { id: 'agents', label: 'Agents', icon: '⚙' },
    { id: 'registry', label: 'Registry', icon: '▤' },
    { id: 'settings', label: 'Settings', icon: '⊛' },
  ];

  function navigate(view: View) {
    currentView.set(view);
  }
</script>

<aside class="w-56 bg-sidebar text-sidebar-text flex flex-col h-full">
  <div class="p-4 border-b border-white/10">
    <h1 class="text-lg font-semibold tracking-tight">Context Harness</h1>
    {#if $workspace}
      <p class="text-xs text-white/50 mt-1 truncate">{$workspace.name}</p>
    {/if}
  </div>

  <nav class="flex-1 py-2">
    {#each navItems as item}
      <button
        class="w-full text-left px-4 py-2.5 text-sm flex items-center gap-3 transition-colors
               hover:bg-sidebar-active
               {$currentView === item.id ? 'bg-sidebar-active font-medium' : 'text-white/70'}"
        onclick={() => navigate(item.id)}
      >
        <span class="text-base w-5 text-center">{item.icon}</span>
        {item.label}
      </button>
    {/each}
  </nav>

  <div class="p-4 border-t border-white/10">
    <button
      class="w-full text-left text-sm text-white/50 hover:text-white/80 transition-colors"
      onclick={closeWorkspace}
    >
      Close Workspace
    </button>
  </div>
</aside>
