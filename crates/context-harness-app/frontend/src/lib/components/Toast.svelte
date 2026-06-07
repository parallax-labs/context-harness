<script lang="ts">
  import { error } from '../stores/app';
</script>

{#if $error}
  <div class="fixed bottom-4 right-4 bg-error text-white px-4 py-3 rounded-lg shadow-lg max-w-md z-50 animate-slide-up">
    <div class="flex items-start gap-2">
      <span class="text-lg">⚠</span>
      <p class="text-sm">{$error}</p>
      <button
        class="ml-auto text-white/70 hover:text-white"
        onclick={() => error.set(null)}
      >×</button>
    </div>
  </div>
{/if}

<style>
  @keyframes slide-up {
    from { transform: translateY(1rem); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }
  .animate-slide-up {
    animation: slide-up 0.2s ease-out;
  }
</style>
