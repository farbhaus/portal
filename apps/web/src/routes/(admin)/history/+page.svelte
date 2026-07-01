<script lang="ts">
  import { goto } from "$app/navigation";
  import { PageHeader, EmptyState } from "$lib/components";
  import { formatBytes } from "$lib/format";
  import type { Transfer } from "./+page.server";

  let { data } = $props();
  const items = $derived(data.items as Transfer[]);

  const kindMeta: Record<string, { label: string; cls: string }> = {
    upload: { label: "↑ Upload", cls: "text-info" },
    download: { label: "↓ Download", cls: "text-accent" },
  };
  const fmtBytes = (n: number | null): string => (n == null ? "—" : formatBytes(n));

  function nav(kind: string, offset: number) {
    const p = new URLSearchParams();
    if (kind !== "all") p.set("kind", kind);
    if (offset > 0) p.set("offset", String(offset));
    const qs = p.toString();
    goto(qs ? `/history?${qs}` : "/history");
  }
  function onFilter(e: Event) {
    nav((e.target as HTMLSelectElement).value, 0);
  }
</script>

<div class="space-y-5">
  <PageHeader title="Transfer history" subtitle="Every completed upload and download, newest first.">
    {#snippet actions()}
      <select onchange={onFilter} class="rounded-md border border-border bg-surface-2 px-2 py-1.5 text-sm">
        <option value="all" selected={data.kind === "all"}>All transfers</option>
        <option value="upload" selected={data.kind === "upload"}>Uploads</option>
        <option value="download" selected={data.kind === "download"}>Downloads</option>
      </select>
      <a href="/activity" class="rounded-md border border-border px-2 py-1.5 text-sm text-muted hover:text-text">Admin activity →</a>
    {/snippet}
  </PageHeader>

  {#if items.length === 0}
    <EmptyState message="No transfers yet." />
  {:else}
    <div class="overflow-hidden rounded-card border border-border bg-surface">
      <div class="divide-y divide-border/60">
        {#each items as r (r.kind + r.id)}
          <div class="flex items-center justify-between gap-4 px-5 py-2.5 text-sm">
            <div class="flex min-w-0 items-center gap-3">
              <span class="w-20 shrink-0 text-xs font-medium {kindMeta[r.kind]?.cls}">{kindMeta[r.kind]?.label ?? r.kind}</span>
              <span class="truncate">{r.label ?? "—"}</span>
              {#if r.who}<span class="hidden truncate text-muted sm:inline">· {r.who}</span>{/if}
            </div>
            <div class="flex shrink-0 items-center gap-3">
              <span class="hidden text-xs text-faint sm:inline">{fmtBytes(r.bytes)}</span>
              <span class="text-right text-xs whitespace-nowrap text-faint">{new Date(r.at).toLocaleString()}</span>
            </div>
          </div>
        {/each}
      </div>
    </div>

    <div class="flex items-center justify-between">
      <button
        onclick={() => nav(data.kind, Math.max(0, data.offset - data.pageSize))}
        disabled={data.offset === 0}
        class="rounded-md border border-border px-3 py-1.5 text-sm text-muted hover:text-text disabled:opacity-40"
      >← Newer</button>
      <span class="text-xs text-faint">{data.offset + 1}–{data.offset + items.length}</span>
      <button
        onclick={() => nav(data.kind, data.offset + data.pageSize)}
        disabled={!data.hasMore}
        class="rounded-md border border-border px-3 py-1.5 text-sm text-muted hover:text-text disabled:opacity-40"
      >Older →</button>
    </div>
  {/if}
</div>
