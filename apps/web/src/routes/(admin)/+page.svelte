<script lang="ts">
  import { invalidateAll } from "$app/navigation";
  import { Card, Button, StatusPill, CopyButton } from "$lib/components";
  import { formatBytes } from "$lib/format";
  import type { RecentTransfer } from "./+page.server";

  let { data } = $props();

  const active = $derived(data.transfers.active_uploads);
  const recent = $derived(data.transfers.recent as RecentTransfer[]);
  const health = $derived(data.transfers.sync_health);

  let refreshing = $state(false);

  // Live-refresh the feed while something is uploading, then go quiet.
  $effect(() => {
    if (active.length === 0) return;
    const t = setInterval(async () => {
      refreshing = true;
      await invalidateAll();
      refreshing = false;
    }, 4000);
    return () => clearInterval(t);
  });

  const fmtBytes = (n: number | null): string => (n == null ? "—" : formatBytes(n));

  function timeAgo(iso: string): string {
    const s = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
    if (s < 60) return "just now";
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    return `${Math.floor(h / 24)}d ago`;
  }

  function shortUrl(url: string): string {
    try { return new URL(url).pathname; } catch { return url; }
  }

  const kindMeta: Record<string, { label: string; cls: string }> = {
    upload: { label: "↑ Upload", cls: "text-info" },
    download: { label: "↓ Download", cls: "text-accent" },
    sync: { label: "⇄ Sync", cls: "text-muted" },
  };
</script>

<div class="space-y-6">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <h1 class="text-xl font-semibold tracking-tight">Dashboard</h1>
    <a
      href="/settings"
      class="inline-flex shrink-0 items-center gap-2 rounded-full border border-border px-3 py-1 text-xs font-medium {data.connected
        ? 'text-success'
        : 'text-warning'}"
    >
      <span class="h-1.5 w-1.5 rounded-full {data.connected ? 'bg-success' : 'bg-warning'}"></span>
      Frame.io {data.connected ? "connected" : "not connected"}
    </a>
  </div>

  <!-- Live transfers -->
  {#if active.length > 0}
    <Card class="border-accent/30">
      <div class="mb-3 flex items-center gap-2">
        <span class="h-2 w-2 rounded-full bg-accent animate-pulse-dot"></span>
        <h2 class="text-sm font-semibold">Uploading now</h2>
        <span class="text-xs text-faint">({active.length})</span>
      </div>
      <div class="space-y-2">
        {#each active as u (u.id)}
          <div class="flex items-center justify-between gap-4 text-sm">
            <div class="min-w-0">
              <span class="font-medium">{u.brand ?? "Upload"}</span>
              {#if u.who}<span class="text-muted"> · {u.who}</span>{/if}
            </div>
            <div class="shrink-0 text-xs text-muted">
              {fmtBytes(u.total_bytes)}{#if u.started_at} · started {timeAgo(u.started_at)}{/if}
            </div>
          </div>
        {/each}
      </div>
    </Card>
  {/if}

  <!-- Sync health strip -->
  <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
    {#if refreshing}
      {#each Array(4) as _, i (i)}
        <div class="rounded-card border border-border bg-surface px-4 py-3">
          <div class="skeleton h-8 w-10 mb-1.5"></div>
          <div class="skeleton h-3 w-20"></div>
        </div>
      {/each}
    {:else}
      {#snippet stat(label: string, value: number, tone: string)}
        <div class="rounded-card border border-border bg-surface px-4 py-3">
          <div class="text-2xl font-semibold {tone}">{value}</div>
          <div class="mt-0.5 text-xs text-muted">{label}</div>
        </div>
      {/snippet}
      {@render stat("Synced (24h)", health.done_24h, "text-text")}
      {@render stat("In progress", health.running, health.running ? "text-info" : "text-text")}
      {@render stat("Waiting", health.waiting, health.waiting ? "text-warning" : "text-text")}
      {@render stat("Failed", health.dead_letter, health.dead_letter ? "text-danger" : "text-text")}
    {/if}
  </div>

  {#if health.dead_letter > 0}
    <a href="/sync" class="block rounded-card border border-danger/40 bg-danger/10 px-4 py-2.5 text-sm text-danger hover:bg-danger/15">
      {health.dead_letter} sync job{health.dead_letter > 1 ? "s" : ""} failed and need attention →
    </a>
  {/if}

  <!-- Quick links: upload + download, side by side on desktop, stacked on mobile -->
  {#snippet linkList(title: string, href: string, links: { id: string; public_url: string; brand_display_name?: string | null }[])}
    <Card padded={false}>
      <div class="flex items-center justify-between border-b border-border px-5 py-3">
        <h2 class="text-sm font-semibold">{title}</h2>
        <a href={href} class="text-xs text-muted hover:text-text">All →</a>
      </div>
      {#if links.length === 0}
        <p class="px-5 py-6 text-center text-xs text-muted">No active links.</p>
      {:else}
        <div class="divide-y divide-border/60">
          {#each links.slice(0, 5) as l (l.id)}
            <div class="flex items-center justify-between gap-2 px-5 py-2.5">
              <div class="min-w-0">
                <div class="truncate text-sm font-medium">{l.brand_display_name ?? "Untitled"}</div>
                <CopyButton value={l.public_url} label={shortUrl(l.public_url)} class="text-xs" />
              </div>
              <Button href={l.public_url} target="_blank" rel="noopener" variant="ghost" size="sm">Open</Button>
            </div>
          {/each}
        </div>
      {/if}
    </Card>
  {/snippet}

  <div class="grid gap-6 sm:grid-cols-2">
    {@render linkList("Upload links", "/upload-links", data.activeUpload)}
    {@render linkList("Download links", "/download-links", data.activeDownload)}
  </div>

  <!-- Recent feed -->
  <Card padded={false}>
    <div class="flex items-center justify-between border-b border-border px-5 py-3">
      <h2 class="text-sm font-semibold">Recent transfers</h2>
      <a href="/activity" class="text-xs text-muted hover:text-text">Activity log →</a>
    </div>
    {#if recent.length === 0}
      <p class="px-5 py-10 text-center text-sm text-muted">Nothing yet. Transfers will show up here.</p>
    {:else}
      <div class="max-h-[calc(100vh-26rem)] divide-y divide-border/60 overflow-y-auto">
        {#each recent as r (r.kind + r.id)}
          <div class="flex items-center justify-between gap-4 px-5 py-2.5 text-sm">
            <div class="flex min-w-0 items-center gap-3">
              <span class="w-20 shrink-0 text-xs font-medium {kindMeta[r.kind]?.cls}">{kindMeta[r.kind]?.label}</span>
              <span class="truncate">{r.label ?? "—"}</span>
              {#if r.who}<span class="hidden truncate text-muted sm:inline">· {r.who}</span>{/if}
            </div>
            <div class="flex shrink-0 items-center gap-2">
              <span class="hidden text-xs text-faint sm:inline">{fmtBytes(r.bytes)}</span>
              {#if r.kind === "sync"}<span class="hidden sm:block"><StatusPill status={r.status} /></span>{/if}
              <span class="text-right text-xs text-faint">{timeAgo(r.at)}</span>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </Card>
</div>
