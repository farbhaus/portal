<script lang="ts">
  import type { Snippet } from "svelte";

  // Bordered dark table shell. Pass column headers; render <tr> rows in the
  // default slot. Use the exported `td`/`row` classes for consistent cells.
  let {
    columns,
    children,
  }: { columns: (string | { label: string; class?: string })[]; children: Snippet } = $props();

  const norm = $derived(
    columns.map((c) => (typeof c === "string" ? { label: c, class: "" } : c)),
  );
</script>

<div class="overflow-hidden rounded-card border border-border bg-surface">
  <table class="w-full text-sm">
    <thead class="border-b border-border text-left text-xs tracking-wide text-faint uppercase">
      <tr>
        {#each norm as col, i (i)}
          <th class="px-4 py-2.5 font-medium {col.class ?? ''}">{col.label}</th>
        {/each}
      </tr>
    </thead>
    <tbody class="divide-y divide-border/60">
      {@render children()}
    </tbody>
  </table>
</div>
