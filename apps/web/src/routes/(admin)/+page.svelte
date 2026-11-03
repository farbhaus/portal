<script lang="ts">
  import { invalidateAll } from "$app/navigation";
  import { onMount } from "svelte";
  import { Icon } from "$lib/components";
  import { formatBytes } from "$lib/format";
  import { onLiveChange, refreshOnChange } from "$lib/live.svelte";
  import { fmtDuration, fmtSpeed, timeAgo, whenFull, type LinkItem } from "$lib/links";
  import { errorDetail, toast } from "$lib/toast.svelte";
  import type { ActiveUpload, HistoryItem } from "./+page";

  let { data } = $props();
  const uploads = $derived(data.transfers.active_uploads as ActiveUpload[]);
  const links = $derived(data.links as LinkItem[]);

  // The page reloads on every live change; a slow timer only if the event stream can't connect.
  $effect(() => refreshOnChange(uploads.length ? 8000 : 30_000));

  // ── live rows ───────────────────────────────────────────────────────────
  function uploadProgress(u: ActiveUpload) {
    const done = u.uploaded_bytes ?? 0;
    const pct = u.total_bytes ? Math.min(100, (100 * done) / u.total_bytes) : null;
    const secs = u.started_at && u.last_activity_at ? (Date.parse(u.last_activity_at) - Date.parse(u.started_at)) / 1000 : 0;
    const bps = secs >= 1 && done > 0 ? done / secs : null;
    const eta = bps && u.total_bytes && u.total_bytes > done ? fmtDuration((u.total_bytes - done) / bps) : null;
    return { done, pct, bps, eta };
  }

  // ── expiring soon ───────────────────────────────────────────────────────
  const SOON = 3 * 86_400_000;
  const expiring = $derived(
    links
      .filter((l) => l.state === "ok" && l.expires_at && Date.parse(l.expires_at) - Date.now() < SOON)
      .sort((a, b) => Date.parse(a.expires_at!) - Date.parse(b.expires_at!)),
  );
  function untilText(iso: string): string {
    const ms = Date.parse(iso) - Date.now();
    if (ms < 3_600_000) return `in ${Math.max(1, Math.round(ms / 60_000))}m`;
    if (ms < 86_400_000) return `in ${Math.round(ms / 3_600_000)}h`;
    const days = Math.round(ms / 86_400_000);
    return days === 1 ? "tomorrow" : `in ${days} days`;
  }
  async function extend(l: LinkItem) {
    const end = new Date();
    end.setDate(end.getDate() + 7);
    end.setHours(23, 59, 0, 0);
    const res = await fetch(`/api/${l.kind === "upload" ? "upload-links" : "download-links"}/${l.id}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ expires_at: end.toISOString() }),
    });
    if (!res.ok) return toast(await errorDetail(res, "Could not extend"), { kind: "error" });
    toast(`“${l.name}” is open for 7 more days`);
    await invalidateAll();
  }

  // ── history ─────────────────────────────────────────────────────────────
  let q = $state("");
  let kind = $state<"all" | "upload" | "download">("all");
  let items = $state<HistoryItem[]>([]);
  let hasMore = $state(false);
  let loading = $state(true);
  let loadingMore = $state(false);
  let seq = 0;
  const PAGE = 40;

  async function fetchHistory(limit: number, offset = 0) {
    const p = new URLSearchParams({ kind, limit: String(limit), offset: String(offset) });
    if (q.trim()) p.set("q", q.trim());
    const res = await fetch(`/api/dashboard/history?${p}`);
    if (!res.ok) throw new Error("Could not load history");
    return (await res.json()) as { items: HistoryItem[]; has_more: boolean };
  }
  async function reloadHistory(keep = false) {
    const my = ++seq;
    if (!keep) loading = true;
    try {
      const r = await fetchHistory(Math.max(PAGE, keep ? items.length : 0));
      if (my !== seq) return;
      items = r.items;
      hasMore = r.has_more;
    } catch {
      /* the next change retries */
    } finally {
      if (my === seq) loading = false;
    }
  }
  async function loadMore() {
    loadingMore = true;
    try {
      const r = await fetchHistory(PAGE, items.length);
      items = [...items, ...r.items];
      hasMore = r.has_more;
    } finally {
      loadingMore = false;
    }
  }
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  function onSearch() {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => void reloadHistory(), 250);
  }
  onMount(() => {
    void reloadHistory();
    return onLiveChange(() => void reloadHistory(true));
  });

  async function exportCsv() {
    const r = await fetchHistory(1000);
    const cell = (v: unknown) => {
      let s = v == null ? "" : String(v);
      // Names and file names come from clients: a leading = + - @ would run as a formula in a
      // spreadsheet, so it's quoted as text.
      if (typeof v === "string" && /^[=+\-@\t\r]/.test(s)) s = `'${s}`;
      return /[",\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
    };
    const kindName = { upload: "Received", download: "Sent" } as const;
    const lines = [
      ["When", "Type", "Name", "Project", "Who", "Files", "Bytes", "Status"].join(","),
      ...r.items.map((i) => [i.at, kindName[i.kind], i.label, i.project ?? "", i.kind === "download" && i.via ? `${i.who ?? ""} via ${i.via}` : i.who, i.files ?? "", i.bytes ?? "", statusWord(i)].map(cell).join(",")),
    ];
    const blob = new Blob([lines.join("\n")], { type: "text/csv" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `portal-history-${new Date().toISOString().slice(0, 10)}.csv`;
    a.click();
    URL.revokeObjectURL(a.href);
    toast(`Exported ${r.items.length} row${r.items.length === 1 ? "" : "s"}${r.has_more ? " (the newest 1000)" : ""}`);
  }

  function statusWord(i: HistoryItem): string {
    if (i.kind === "upload") return i.status === "completed" ? "Received" : "Didn't finish";
    return i.status === "completed" ? "Downloaded" : "Started";
  }
  const statusTone = (i: HistoryItem) => (i.status === "incomplete" || i.status === "in_progress" ? "text-warning" : "text-muted");
  const kindIcon = { upload: "receive", download: "send" } as const;
  const href = (i: HistoryItem) =>
    !i.link_id ? null : i.kind === "upload" ? `/upload-links/${i.link_id}` : `/download-links/${i.link_id}`;

  const quiet = $derived(!uploads.length);
</script>

<div class="space-y-6 text-[13px]">
  <div class="flex flex-wrap items-center gap-3">
    <h1 class="text-xl font-semibold tracking-tight">Dashboard</h1>
    <span class="flex-1"></span>
    <a href="/settings" class="inline-flex items-center gap-2 rounded-full border border-border px-3 py-1 text-xs font-medium {data.connected ? 'text-success' : 'text-warning'}">
      <span class="h-1.5 w-1.5 rounded-full {data.connected ? 'bg-success' : 'bg-warning'}"></span>
      Frame.io {data.connected ? "connected" : "not connected"}
    </a>
  </div>

  <!-- now -->
  <section>
    <div class="mb-2 flex items-center gap-2">
      {#if !quiet}<span class="live-dot"></span>{/if}
      <h2 class="text-sm font-semibold">{quiet ? "Nothing moving right now" : "Happening now"}</h2>
    </div>
    {#if !quiet}
      <div class="overflow-hidden rounded-xl border border-border bg-surface">
        {#each uploads as u (u.id)}
          {@const p = uploadProgress(u)}
          <a href={u.link_id ? `/upload-links/${u.link_id}` : "/links"} class="grid grid-cols-[30px_minmax(0,1fr)] items-center gap-x-3 gap-y-1.5 border-b border-border px-4 py-3 last:border-b-0 hover:bg-white/[0.02] md:grid-cols-[30px_minmax(0,1.3fr)_minmax(140px,1fr)_auto]">
            <span class="grid h-[30px] w-[30px] place-items-center rounded-md border border-border bg-surface-2 text-body"><Icon name="receive" /></span>
            <span class="min-w-0">
              <span class="block truncate font-medium text-text">{u.brand ?? "Upload"}</span>
              <span class="block truncate text-xs text-faint">{u.who ?? "Someone"} is uploading</span>
            </span>
            <span class="col-span-2 grid gap-1 md:col-span-1">
              <span class="relative h-1 overflow-hidden rounded-full bg-surface-3"><i class="absolute inset-y-0 left-0 rounded-full bg-accent transition-[width] duration-500" style="width:{p.pct ?? 6}%"></i></span>
              <span class="tnum text-xs text-faint">{formatBytes(p.done)}{u.total_bytes ? ` of ${formatBytes(u.total_bytes)}` : ""}</span>
            </span>
            <span class="tnum col-span-2 text-xs text-muted md:col-span-1 md:text-right">{fmtSpeed(p.bps)}{p.eta ? ` · ` : ""}{#if p.eta}<span class="text-text">{p.eta} left</span>{/if}</span>
          </a>
        {/each}
      </div>
    {/if}
  </section>

  <!-- expiring soon -->
  {#if expiring.length}
    <section>
      <h2 class="mb-2 text-sm font-semibold">Expiring soon</h2>
      <div class="overflow-hidden rounded-xl border border-border bg-surface">
        {#each expiring as l (l.id)}
          <div class="flex items-center gap-3 border-b border-border px-4 py-2.5 last:border-b-0">
            <span class="grid h-[30px] w-[30px] shrink-0 place-items-center rounded-md border border-border bg-surface-2 text-body"><Icon name={l.kind === "upload" ? "receive" : "send"} /></span>
            <a href={l.kind === "upload" ? `/upload-links/${l.id}` : `/download-links/${l.id}`} class="min-w-0 flex-1 hover:text-accent-hover">
              <span class="block truncate font-medium text-text">{l.name}</span>
              <span class="tnum block text-xs text-warning sm:hidden">Closes {untilText(l.expires_at!)}</span>
            </a>
            <span class="tnum hidden shrink-0 text-warning sm:inline" title={whenFull(l.expires_at)}>Closes {untilText(l.expires_at!)}</span>
            <button type="button" class="h-7 shrink-0 rounded-md border border-border px-2.5 text-xs font-medium text-text hover:bg-surface-3" onclick={() => extend(l)}>Extend 7 days</button>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <!-- history -->
  <section id="history" class="scroll-mt-24">
    <div class="mb-2 flex flex-wrap items-center gap-2.5">
      <h2 class="mr-2 text-sm font-semibold">History</h2>
      <label class="relative flex w-full items-center sm:w-64">
        <span class="pointer-events-none absolute left-2.5 text-faint"><Icon name="search" /></span>
        <input bind:value={q} oninput={onSearch} type="text" placeholder="Search names, people, files" aria-label="Search history" class="h-8 w-full rounded-md border border-border bg-bg pr-2 pl-8 outline-none focus:border-accent" />
      </label>
      <div class="inline-flex gap-0.5 rounded-md border border-border bg-surface p-0.5" role="group" aria-label="Type">
        {#each [["all", "All"], ["upload", "Received"], ["download", "Sent"]] as [k, l] (k)}
          <button type="button" class="h-[26px] rounded px-2.5 font-medium {kind === k ? 'bg-surface-3 text-text' : 'text-muted hover:text-text'}" onclick={() => { kind = k as typeof kind; void reloadHistory(); }}>{l}</button>
        {/each}
      </div>
      <span class="flex-1"></span>
      <button type="button" class="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-surface-2 px-3 font-medium text-text hover:bg-surface-3" onclick={exportCsv}>Export CSV</button>
      <a href="/activity" class="text-xs text-muted hover:text-text">Admin log</a>
    </div>

    <div class="overflow-hidden rounded-xl border border-border bg-surface">
      {#if loading}
        <p class="px-4 py-10 text-center text-faint">Loading…</p>
      {:else}
        {#each items as i (i.kind + i.id)}
          {@const to = href(i)}
          <svelte:element
            this={to ? "a" : "div"}
            href={to}
            class="grid grid-cols-[30px_minmax(0,1fr)_auto] items-center gap-x-3 border-b border-border px-4 py-2 last:border-b-0 sm:grid-cols-[30px_minmax(0,1.4fr)_minmax(0,1fr)_80px_110px_96px] {to ? 'hover:bg-white/[0.02]' : ''}"
          >
            <span class="grid h-[30px] w-[30px] place-items-center rounded-md border border-border bg-surface-2 text-body"><Icon name={kindIcon[i.kind]} /></span>
            <span class="min-w-0">
              <span class="block truncate text-text" title={[i.label, i.project].filter(Boolean).join(" · ")}>{i.label ?? "—"}{#if i.project}<span class="text-faint"> · {i.project}</span>{/if}</span>
              <span class="block truncate text-xs text-faint sm:hidden">{i.who ?? ""}</span>
            </span>
            <span class="hidden min-w-0 truncate text-muted sm:block" title={i.who ?? ""}>{i.who ?? "—"}{#if i.kind === "download" && i.via}<span class="text-faint"> · {i.via}</span>{/if}</span>
            <span class="tnum hidden text-right text-muted sm:block">{i.bytes != null ? formatBytes(i.bytes) : ""}</span>
            <span class="hidden truncate text-xs sm:block {statusTone(i)}">{statusWord(i)}{i.kind === "upload" && i.files ? ` · ${i.files} file${i.files === 1 ? "" : "s"}` : ""}</span>
            <span class="text-right text-xs text-faint" title={whenFull(i.at)}>{timeAgo(i.at)}</span>
          </svelte:element>
        {:else}
          <p class="px-4 py-10 text-center text-faint">{q.trim() ? `Nothing matches “${q}”.` : "Nothing here yet."}</p>
        {/each}
      {/if}
    </div>
    {#if hasMore && !loading}
      <div class="mt-3 text-center">
        <button type="button" class="h-8 rounded-md border border-border px-4 font-medium text-text hover:bg-surface-3 disabled:opacity-50" disabled={loadingMore} onclick={loadMore}>{loadingMore ? "Loading…" : "Show more"}</button>
      </div>
    {/if}
  </section>
</div>
