<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    padded = true,
    class: klass = "",
    children,
  }: { padded?: boolean; class?: string; children: Snippet } = $props();
</script>

<div
  class="rounded-card border border-border bg-surface shadow-sm {padded ? 'p-5' : ''} {klass}"
>
  {@render children()}
</div>
