<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { showError } from '../stores/app';
  import { onMount, onDestroy } from 'svelte';

  interface DocumentItem {
    id: string;
    title: string | null;
    source: string;
    source_id: string;
    source_url: string | null;
    updated_at: string;
    content_type: string;
  }

  interface ChunkInfo {
    id: string;
    index: number;
    text: string;
    content_hash: string;
    has_embedding: boolean;
  }

  interface ImportResult {
    imported: number;
    failed: number;
    errors: string[];
    connectors_created: string[];
  }

  let documents = $state<DocumentItem[]>([]);
  let total = $state(0);
  let offset = $state(0);
  let selectedDocId = $state<string | null>(null);
  let selectedDoc = $state<any>(null);
  let chunks = $state<ChunkInfo[]>([]);
  let loadingDocs = $state(false);
  let dragging = $state(false);
  let importing = $state(false);
  let importMessage = $state('');
  const limit = 50;

  let unlistenDragDrop: (() => void) | null = null;

  onMount(async () => {
    loadDocuments();

    const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'enter' || event.payload.type === 'over') {
        dragging = true;
      } else if (event.payload.type === 'drop') {
        dragging = false;
        const paths = event.payload.paths;
        if (paths && paths.length > 0) {
          importFiles(paths);
        }
      } else {
        dragging = false;
      }
    });

    unlistenDragDrop = unlisten;
  });

  onDestroy(() => {
    if (unlistenDragDrop) {
      unlistenDragDrop();
    }
  });

  async function loadDocuments() {
    loadingDocs = true;
    try {
      const res = await invoke<{ documents: DocumentItem[]; total: number }>('document_list', {
        limit,
        offset,
      });
      documents = res.documents;
      total = res.total;
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to load documents');
    } finally {
      loadingDocs = false;
    }
  }

  async function importFiles(paths: string[]) {
    importing = true;
    importMessage = `Importing ${paths.length} file${paths.length > 1 ? 's' : ''}...`;
    try {
      const result = await invoke<ImportResult>('document_import', { paths });
      const parts: string[] = [];
      parts.push(`Imported ${result.imported} file${result.imported !== 1 ? 's' : ''}`);
      if (result.failed > 0) parts.push(`${result.failed} failed`);
      if (result.connectors_created.length > 0) {
        parts.push(`${result.connectors_created.length} connector${result.connectors_created.length !== 1 ? 's' : ''} created`);
      }
      importMessage = parts.join(' · ');
      if (result.errors.length > 0) {
        showError(result.errors.join('\n'));
      }
      await loadDocuments();
      setTimeout(() => { importMessage = ''; }, 4000);
    } catch (e: any) {
      importMessage = '';
      showError(typeof e === 'string' ? e : 'Import failed');
    } finally {
      importing = false;
    }
  }

  async function selectDocument(id: string) {
    selectedDocId = id;
    try {
      selectedDoc = await invoke('document_get', { id });
      chunks = await invoke<ChunkInfo[]>('document_chunks', { documentId: id });
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to load document');
    }
  }

  function nextPage() {
    offset += limit;
    loadDocuments();
  }

  function prevPage() {
    offset = Math.max(0, offset - limit);
    loadDocuments();
  }

  function fileIcon(contentType: string): string {
    if (contentType === 'application/pdf') return '📄';
    if (contentType.includes('wordprocessing')) return '📝';
    if (contentType.includes('presentation')) return '📊';
    if (contentType.includes('spreadsheet')) return '📈';
    if (contentType === 'text/markdown') return '📑';
    return '📃';
  }
</script>

{#if dragging}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-primary/10 backdrop-blur-sm border-4 border-dashed border-primary rounded-xl m-2 pointer-events-none">
    <div class="text-center">
      <div class="text-5xl mb-3">📥</div>
      <div class="text-xl font-semibold text-primary">Drop files to import</div>
      <div class="text-sm text-text-muted mt-1">PDF, DOCX, PPTX, XLSX, Markdown, text, code</div>
    </div>
  </div>
{/if}

<div class="p-6 h-full flex gap-4">
  <div class="w-80 flex-shrink-0 flex flex-col">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-2xl font-bold text-text">Documents</h2>
    </div>

    {#if importMessage}
      <div class="mb-3 px-3 py-2 rounded-lg text-sm {importing ? 'bg-primary/10 text-primary' : 'bg-success/10 text-success'}">
        {#if importing}
          <span class="inline-block animate-spin mr-1.5">⏳</span>
        {/if}
        {importMessage}
      </div>
    {/if}

    <div class="text-sm text-text-muted mb-3">{total} documents</div>

    <div class="flex-1 overflow-auto space-y-1">
      {#if loadingDocs}
        <div class="text-center py-8 text-text-muted">Loading...</div>
      {:else if documents.length === 0}
        <div class="text-center py-12 text-text-muted">
          <div class="text-4xl mb-3">📥</div>
          <div class="text-sm font-medium mb-1">No documents yet</div>
          <div class="text-xs">Drag and drop files here to import them</div>
        </div>
      {:else}
        {#each documents as doc}
          <button
            class="w-full text-left px-3 py-2.5 rounded-lg text-sm transition-colors
                   {selectedDocId === doc.id ? 'bg-primary/10 border border-primary/20' : 'hover:bg-surface-alt'}"
            onclick={() => selectDocument(doc.id)}
          >
            <div class="flex items-center gap-2">
              <span class="text-base flex-shrink-0">{fileIcon(doc.content_type)}</span>
              <div class="min-w-0 flex-1">
                <div class="font-medium text-text truncate">
                  {doc.title ?? doc.source_id}
                </div>
                <div class="text-xs text-text-muted mt-0.5">
                  {doc.source}{doc.source === 'import' ? '' : `:${doc.source_id}`}
                </div>
              </div>
            </div>
          </button>
        {/each}
      {/if}
    </div>

    {#if total > limit}
      <div class="flex items-center justify-between pt-3 border-t border-border">
        <button
          class="text-sm text-primary disabled:text-text-muted"
          onclick={prevPage}
          disabled={offset === 0}
        >Previous</button>
        <span class="text-xs text-text-muted">
          {offset + 1}–{Math.min(offset + limit, total)} of {total}
        </span>
        <button
          class="text-sm text-primary disabled:text-text-muted"
          onclick={nextPage}
          disabled={offset + limit >= total}
        >Next</button>
      </div>
    {/if}
  </div>

  <div class="flex-1 overflow-auto">
    {#if selectedDoc}
      <div class="bg-surface border border-border rounded-xl p-6">
        <h3 class="text-lg font-semibold text-text">
          {selectedDoc.title ?? selectedDoc.source_id ?? 'Document'}
        </h3>
        <div class="text-xs text-text-muted mt-1 mb-4">
          Source: {selectedDoc.source} · Type: {selectedDoc.content_type} · Updated: {selectedDoc.updated_at}
        </div>
        <div class="prose prose-sm max-w-none text-text whitespace-pre-wrap mb-6">
          {selectedDoc.body ?? 'No content'}
        </div>

        {#if chunks.length > 0}
          <div class="border-t border-border pt-4">
            <h4 class="text-sm font-medium text-text-muted mb-3">
              Chunks ({chunks.length})
            </h4>
            {#each chunks as chunk}
              <div class="mb-3 p-3 bg-surface-alt rounded-lg text-sm">
                <div class="flex items-center gap-2 mb-1">
                  <span class="text-xs text-text-muted">#{chunk.index}</span>
                  <span class="text-xs px-1.5 py-0.5 rounded {chunk.has_embedding ? 'bg-success/10 text-success' : 'bg-warning/10 text-warning'}">
                    {chunk.has_embedding ? 'Embedded' : 'Pending'}
                  </span>
                </div>
                <div class="text-text whitespace-pre-wrap">{chunk.text}</div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <div class="h-full flex flex-col items-center justify-center text-text-muted gap-3">
        <div class="text-4xl">📥</div>
        <div>Select a document or drag and drop files to import</div>
        <div class="text-xs">Supports PDF, DOCX, PPTX, XLSX, Markdown, plain text, and source code</div>
      </div>
    {/if}
  </div>
</div>
