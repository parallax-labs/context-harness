<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { showError, currentView } from '../stores/app';

  interface SearchResult {
    id: string;
    score: number;
    title: string | null;
    source: string;
    source_id: string;
    updated_at: string;
    snippet: string;
    source_url: string | null;
  }

  let query = $state('');
  let mode = $state('hybrid');
  let sourceFilter = $state('');
  let results = $state<SearchResult[]>([]);
  let searching = $state(false);
  let hasSearched = $state(false);
  let selectedDoc = $state<any>(null);

  async function handleSearch() {
    if (!query.trim()) return;
    searching = true;
    hasSearched = true;
    try {
      results = await invoke<SearchResult[]>('search', {
        query: query.trim(),
        mode,
        limit: 20,
        source: sourceFilter || null,
      });
    } catch (e: any) {
      showError(typeof e === 'string' ? e : e.message || 'Search failed');
      results = [];
    } finally {
      searching = false;
    }
  }

  async function viewDocument(id: string) {
    try {
      selectedDoc = await invoke('document_get', { id });
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to load document');
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') handleSearch();
  }
</script>

<div class="p-6 h-full flex flex-col">
  <h2 class="text-2xl font-bold text-text mb-4">Search</h2>

  <div class="flex gap-3 mb-4">
    <input
      type="text"
      placeholder="Search your knowledge base..."
      bind:value={query}
      onkeydown={handleKeydown}
      class="flex-1 px-4 py-2.5 border border-border rounded-lg bg-surface text-text
             placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-primary"
    />
    <select
      bind:value={mode}
      class="px-3 py-2.5 border border-border rounded-lg bg-surface text-text"
    >
      <option value="keyword">Keyword</option>
      <option value="semantic">Semantic</option>
      <option value="hybrid">Hybrid</option>
    </select>
    <button
      class="px-5 py-2.5 bg-primary text-white rounded-lg font-medium
             hover:bg-primary-hover transition-colors disabled:opacity-50"
      onclick={handleSearch}
      disabled={searching || !query.trim()}
    >
      {searching ? 'Searching...' : 'Search'}
    </button>
  </div>

  <div class="flex-1 overflow-auto">
    {#if selectedDoc}
      <div class="mb-4">
        <button
          class="text-sm text-primary hover:underline"
          onclick={() => selectedDoc = null}
        >
          ← Back to results
        </button>
      </div>
      <div class="bg-surface border border-border rounded-xl p-6">
        <h3 class="text-lg font-semibold text-text mb-2">
          {selectedDoc.document?.title ?? selectedDoc.document?.source_id ?? 'Document'}
        </h3>
        <div class="text-xs text-text-muted mb-4">
          Source: {selectedDoc.document?.source} · Updated: {selectedDoc.document?.updated_at}
        </div>
        <div class="prose prose-sm max-w-none text-text whitespace-pre-wrap">
          {selectedDoc.document?.body ?? 'No content'}
        </div>
        {#if selectedDoc.chunks?.length > 0}
          <div class="mt-6 border-t border-border pt-4">
            <h4 class="text-sm font-medium text-text-muted mb-3">
              Chunks ({selectedDoc.chunks.length})
            </h4>
            {#each selectedDoc.chunks as chunk, i}
              <div class="mb-3 p-3 bg-surface-alt rounded-lg text-sm text-text">
                <div class="text-xs text-text-muted mb-1">Chunk {chunk.index}</div>
                <div class="whitespace-pre-wrap">{chunk.body}</div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {:else if results.length > 0}
      <div class="space-y-2">
        {#each results as result}
          <button
            class="w-full text-left bg-surface border border-border rounded-xl p-4
                   hover:border-primary/30 transition-colors"
            onclick={() => viewDocument(result.id)}
          >
            <div class="flex items-start justify-between">
              <div class="flex-1 min-w-0">
                <div class="font-medium text-text truncate">
                  {result.title ?? result.source_id}
                </div>
                <div class="text-xs text-text-muted mt-1">
                  {result.source} · Score: {result.score.toFixed(3)}
                </div>
              </div>
            </div>
            <p class="text-sm text-text-muted mt-2 line-clamp-2">{result.snippet}</p>
          </button>
        {/each}
      </div>
    {:else if hasSearched}
      <div class="text-center py-12 text-text-muted">
        No results found. Try a different query or search mode.
      </div>
    {:else}
      <div class="text-center py-12 text-text-muted">
        <p class="text-lg mb-2">Search your knowledge base</p>
        <p class="text-sm">Try keyword, semantic, or hybrid search across all your documents.</p>
        <p class="text-xs mt-4 text-text-muted/60">Tip: Press Ctrl+K / ⌘K anywhere to jump here.</p>
      </div>
    {/if}
  </div>
</div>
