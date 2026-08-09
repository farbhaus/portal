<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    actions = undefined,
  }: { title: string; actions?: Snippet } = $props();
</script>

<div class="flex items-start justify-between gap-4">
  <h1 class="text-xl font-semibold tracking-tight">{title}</h1>
  {#if actions}
    <div class="flex shrink-0 items-center gap-2">{@render actions()}</div>
  {/if}
</div>
