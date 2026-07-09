<script lang="ts">
  // The one way to show a storage location in the admin UI: a Frame.io breadcrumb (root→leaf,
  // leaf emphasized) or a local filesystem path. Pass `onnavigate` to make segments clickable
  // (pickers); omit it for read-only display.
  type Segment = { id?: string; name: string };

  let {
    segments,
    variant = "frameio",
    onnavigate,
    class: cls = "",
  }: {
    segments: Segment[];
    variant?: "frameio" | "local";
    onnavigate?: (index: number) => void;
    class?: string;
  } = $props();
</script>

{#if variant === "local"}
  <span class="break-all font-mono text-muted {cls}">{segments.map((s) => s.name).join("/")}</span>
{:else}
  <span class="inline-flex min-w-0 flex-wrap items-center gap-x-1 {cls}">
    {#each segments as seg, i (seg.id ?? i)}
      {#if i > 0}<span class="shrink-0 text-faint">/</span>{/if}
      {#if onnavigate}
        <button
          type="button"
          onclick={() => onnavigate(i)}
          title={seg.name}
          class="max-w-48 truncate rounded px-1 hover:bg-surface-3 {i === segments.length - 1 ? 'font-medium' : 'text-muted'}"
        >{seg.name}</button>
      {:else}
        <span title={seg.name} class="max-w-48 truncate {i === segments.length - 1 ? 'font-medium' : 'text-muted'}">{seg.name}</span>
      {/if}
    {/each}
  </span>
{/if}
