<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    subtitle = undefined,
    actions = undefined,
  }: { title: string; subtitle?: string; actions?: Snippet } = $props();
</script>

<div class="flex items-start justify-between gap-4">
  <div>
    <h1 class="text-xl font-semibold tracking-tight">{title}</h1>
    {#if subtitle}
      <p class="mt-0.5 text-sm text-muted">{subtitle}</p>
    {/if}
  </div>
  {#if actions}
    <div class="flex shrink-0 items-center gap-2">{@render actions()}</div>
  {/if}
</div>
