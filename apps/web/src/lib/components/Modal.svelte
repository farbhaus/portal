<script lang="ts">
  // Lightweight modal dialog. Bind `open`; click the backdrop or press Escape to close.
  import type { Snippet } from "svelte";

  let {
    open = $bindable(false),
    title = "",
    children,
    footer = undefined,
  }: {
    open?: boolean;
    title?: string;
    children: Snippet;
    footer?: Snippet;
  } = $props();

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") open = false;
  }
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
    onclick={(e) => { if (e.target === e.currentTarget) open = false; }}
    role="presentation"
  >
    <div
      class="w-full max-w-md rounded-card border border-border bg-surface shadow-lg"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      <div class="flex items-center justify-between border-b border-border px-5 py-3">
        <h2 class="text-sm font-semibold">{title}</h2>
        <button onclick={() => (open = false)} class="text-faint hover:text-text" aria-label="Close">✕</button>
      </div>
      <div class="px-5 py-4">
        {@render children()}
      </div>
      {#if footer}
        <div class="flex items-center justify-end gap-2 border-t border-border px-5 py-3">
          {@render footer()}
        </div>
      {/if}
    </div>
  </div>
{/if}
