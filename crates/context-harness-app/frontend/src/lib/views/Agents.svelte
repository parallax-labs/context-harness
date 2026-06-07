<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { showError } from '../stores/app';
  import { onMount } from 'svelte';

  interface AgentInfo {
    name: string;
    description: string;
    tools: string[];
    source: string;
  }

  let agents = $state<AgentInfo[]>([]);
  let selectedAgent = $state<AgentInfo | null>(null);
  let testOutput = $state<any>(null);

  onMount(loadAgents);

  async function loadAgents() {
    try {
      agents = await invoke<AgentInfo[]>('agent_list');
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to load agents');
    }
  }

  async function selectAgent(name: string) {
    try {
      selectedAgent = await invoke<AgentInfo>('agent_get', { name });
      testOutput = null;
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to load agent');
    }
  }

  async function testAgent() {
    if (!selectedAgent) return;
    try {
      testOutput = await invoke('agent_test', {
        name: selectedAgent.name,
        args: {},
      });
    } catch (e: any) {
      showError(typeof e === 'string' ? e : 'Failed to test agent');
    }
  }
</script>

<div class="p-6 h-full flex gap-4">
  <div class="w-72 flex-shrink-0">
    <h2 class="text-2xl font-bold text-text mb-4">Agents</h2>

    {#if agents.length === 0}
      <div class="text-sm text-text-muted py-8 text-center">
        No agents configured. Add agents in your workspace config.
      </div>
    {:else}
      <div class="space-y-1">
        {#each agents as agent}
          <button
            class="w-full text-left px-3 py-2.5 rounded-lg text-sm transition-colors
                   {selectedAgent?.name === agent.name ? 'bg-primary/10 border border-primary/20' : 'hover:bg-surface-alt'}"
            onclick={() => selectAgent(agent.name)}
          >
            <div class="font-medium text-text">{agent.name}</div>
            <div class="text-xs text-text-muted mt-0.5 truncate">{agent.description}</div>
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="flex-1 overflow-auto">
    {#if selectedAgent}
      <div class="bg-surface border border-border rounded-xl p-6">
        <div class="flex items-start justify-between mb-4">
          <div>
            <h3 class="text-lg font-semibold text-text">{selectedAgent.name}</h3>
            <p class="text-sm text-text-muted mt-1">{selectedAgent.description}</p>
          </div>
          <button
            class="px-4 py-2 bg-primary text-white rounded-lg font-medium
                   hover:bg-primary-hover transition-colors"
            onclick={testAgent}
          >
            Test
          </button>
        </div>

        <div class="mb-4">
          <h4 class="text-sm font-medium text-text-muted mb-2">Tools</h4>
          <div class="flex flex-wrap gap-2">
            {#each selectedAgent.tools as tool}
              <span class="px-2 py-1 text-xs bg-surface-alt border border-border rounded-md text-text">
                {tool}
              </span>
            {/each}
          </div>
        </div>

        <div class="text-xs text-text-muted">
          Source: {selectedAgent.source}
        </div>

        {#if testOutput}
          <div class="mt-6 border-t border-border pt-4">
            <h4 class="text-sm font-medium text-text-muted mb-2">Test Output</h4>
            <div class="bg-surface-alt rounded-lg p-4">
              <div class="mb-3">
                <div class="text-xs text-text-muted mb-1">System Prompt</div>
                <div class="text-sm text-text whitespace-pre-wrap bg-surface p-3 rounded-md border border-border">
                  {testOutput.system}
                </div>
              </div>
              {#if testOutput.messages?.length > 0}
                <div>
                  <div class="text-xs text-text-muted mb-1">Messages</div>
                  {#each testOutput.messages as msg}
                    <div class="text-sm text-text p-2 bg-surface rounded-md border border-border mb-1">
                      <span class="text-xs text-text-muted">{msg.role}:</span> {msg.content}
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    {:else}
      <div class="h-full flex items-center justify-center text-text-muted">
        Select an agent to view its details and test it.
      </div>
    {/if}
  </div>
</div>
