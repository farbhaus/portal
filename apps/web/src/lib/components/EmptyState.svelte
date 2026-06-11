<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    message,
    children = undefined,
  }: { message: string; children?: Snippet } = $props();
</script>

<div
  class="rounded-card border border-dashed border-border bg-surface/40 p-10 text-center"
>
  <p class="text-sm text-muted">{message}</p>
  {#if children}
    <div class="mt-3 flex justify-center">{@render children()}</div>
  {/if}
</div>
