<script lang="ts">
  import { invalidateAll } from "$app/navigation";
  import { ConfirmButton, Icon } from "$lib/components";
  import { formatBytes } from "$lib/format";
  import { refreshOnChange } from "$lib/live.svelte";
  import {
    expiresText,
    fmtSpeed,
    kindNoun,
    linkPath,
    linkStatus,
    liveProgress,
    sourceSummary,
    timeAgo,
    whenFull,
    type DownloadEvent,
    type LinkItem,
    type UploadSessionRow,
  } from "$lib/links";
  import { copyWithToast, errorDetail, toast } from "$lib/toast.svelte";

  let { data } = $props();
  const links = $derived(data.links as LinkItem[]);

  $effect(() => refreshOnChange(20_000));

  // ── filters ─────────────────────────────────────────────────────────────
  let q = $state("");
  let kind = $state<"all" | "upload" | "download">("all");
  let view = $state<"list" | "project">("list");
  let open = $state(new Set<string>());

  const matches = (l: LinkItem) => {
    const s = q.trim().toLowerCase();
    if (!s) return true;
    return [l.name, l.brand_display_name ?? "", linkPath(l).join(" "), l.activity.last_who ?? "", ...l.activity.live.map((x) => x.who ?? "")]
      .join(" ")
      .toLowerCase()
      .includes(s);
  };
  const searched = $derived(links.filter(matches));
  const shown = $derived(searched.filter((l) => kind === "all" || l.kind === kind));
  const counts = $derived({
    all: searched.length,
    upload: searched.filter((l) => l.kind === "upload").length,
    download: searched.filter((l) => l.kind === "download").length,
  });
  const groups = $derived.by(() => {
    if (view !== "project") return [{ name: "", items: shown }];
    const m = new Map<string, LinkItem[]>();
    for (const l of shown) {
      const p = linkPath(l)[0] ?? "Other";
      if (!m.has(p)) m.set(p, []);
      m.get(p)!.push(l);
    }
    return [...m.entries()].sort((a, b) => a[0].localeCompare(b[0])).map(([name, items]) => ({ name, items }));
  });

  // ── row actions ─────────────────────────────────────────────────────────
  const base = (l: LinkItem) => (l.kind === "upload" ? "/api/upload-links" : "/api/download-links");
  const editHref = (l: LinkItem) => (l.kind === "upload" ? `/upload-links/${l.id}` : `/download-links/${l.id}`);

  async function revoke(l: LinkItem) {
    const res = await fetch(`${base(l)}/${l.id}/revoke`, { method: "POST" });
    if (!res.ok) return toast(await errorDetail(res, "Could not revoke"), { kind: "error" });
    await invalidateAll();
    toast(`“${l.name}” revoked`, {
      action: {
        label: "Undo",
        run: async () => {
          await fetch(`${base(l)}/${l.id}/restore`, { method: "POST" });
          await invalidateAll();
        },
      },
    });
  }

  async function restore(l: LinkItem) {
    await fetch(`${base(l)}/${l.id}/restore`, { method: "POST" });
    await invalidateAll();
    toast(`“${l.name}” is active again`);
  }

  async function extend(l: LinkItem) {
    const end = new Date();
    end.setDate(end.getDate() + 7);
    end.setHours(23, 59, 0, 0);
    const res = await fetch(`${base(l)}/${l.id}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ expires_at: end.toISOString() }),
    });
    if (!res.ok) return toast(await errorDetail(res, "Could not extend"), { kind: "error" });
    await invalidateAll();
    toast(`“${l.name}” is open for 7 more days`);
  }

  async function remove(l: LinkItem) {
    const res = await fetch(`${base(l)}/${l.id}`, { method: "DELETE" });
    if (!res.ok) return toast(await errorDetail(res, "Could not delete"), { kind: "error" });
    await invalidateAll();
    toast(`Deleted “${l.name}”`);
  }

  // ── expanded rows ───────────────────────────────────────────────────────
  type Detail = { sessions?: UploadSessionRow[]; events?: DownloadEvent[]; error?: string };
  let details = $state<Record<string, Detail>>({});

  async function loadDetail(l: LinkItem) {
    try {
      if (l.kind === "upload") {
        const res = await fetch(`/api/upload-links/${l.id}/sessions?limit=8`);
        details[l.id] = res.ok ? { sessions: await res.json() } : { error: "Could not load uploads" };
      } else {
        const res = await fetch(`/api/download-links/${l.id}/events?limit=8`);
        details[l.id] = res.ok ? { events: await res.json() } : { error: "Could not load downloads" };
      }
    } catch {
      details[l.id] = { error: "Could not load activity" };
    }
  }

  function toggle(l: LinkItem) {
    const next = new Set(open);
    if (next.has(l.id)) next.delete(l.id);
    else {
      next.add(l.id);
      void loadDetail(l);
    }
    open = next;
  }

  // Refresh open rows' activity whenever the list reloads (a live change).
  let lastLinks: LinkItem[] | null = null;
  $effect(() => {
    if (links !== lastLinks) {
      lastLinks = links;
      for (const id of open) {
        const l = links.find((x) => x.id === id);
        if (l) void loadDetail(l);
      }
    }
  });

  const who = (n: string | null, e: string | null) => n || e || "Someone";
  const statusTone: Record<string, string> = {
    live: "text-text",
    ok: "text-body",
    muted: "text-muted",
    warn: "text-warning",
    bad: "text-faint",
  };
  const dotTone: Record<string, string> = { ok: "bg-success", muted: "bg-faint", warn: "bg-warning", bad: "bg-danger" };

  const grid =
    "grid items-center gap-x-3.5 grid-cols-[minmax(0,1fr)_auto] md:grid-cols-[minmax(0,1fr)_170px_180px] lg:grid-cols-[minmax(0,1.5fr)_minmax(170px,1.15fr)_92px_176px_180px] xl:grid-cols-[minmax(0,1.6fr)_minmax(190px,1.2fr)_92px_190px_180px]";
</script>

<div class="space-y-4 text-[13px]">
  <div class="flex items-center gap-3">
    <h1 class="text-xl font-semibold tracking-tight">Links</h1>
    <span class="flex-1"></span>
    <a href="/links/new" class="inline-flex h-8 items-center gap-1.5 rounded-md bg-accent px-3 font-medium text-on-accent hover:bg-accent-hover">
      <Icon name="plus" size={15} />New link
    </a>
  </div>

  <!-- toolbar -->
  <div class="flex flex-wrap items-center gap-2.5">
    <label class="relative flex w-full items-center sm:w-72">
      <span class="pointer-events-none absolute left-2.5 text-faint"><Icon name="search" /></span>
      <input bind:value={q} type="text" placeholder="Search links, folders, people" aria-label="Search links" class="h-8 w-full rounded-md border border-border bg-bg pr-2 pl-8 outline-none focus:border-accent" />
    </label>
    <div class="inline-flex gap-0.5 rounded-md border border-border bg-surface p-0.5" role="group" aria-label="Link type">
      {#each [["all", "All"], ["upload", "Receive"], ["download", "Send"]] as [k, l] (k)}
        <button type="button" class="inline-flex h-[26px] items-center gap-1.5 rounded px-2.5 font-medium {kind === k ? 'bg-surface-3 text-text' : 'text-muted hover:text-text'}" onclick={() => (kind = k as typeof kind)}>
          {l}<span class="tnum text-faint">{counts[k as keyof typeof counts]}</span>
        </button>
      {/each}
    </div>
    <span class="flex-1"></span>
    <div class="inline-flex gap-0.5 rounded-md border border-border bg-surface p-0.5" role="group" aria-label="View">
      {#each [["list", "List"], ["project", "By project"]] as [k, l] (k)}
        <button type="button" class="h-[26px] rounded px-2.5 font-medium {view === k ? 'bg-surface-3 text-text' : 'text-muted hover:text-text'}" onclick={() => (view = k as typeof view)}>{l}</button>
      {/each}
    </div>
  </div>

  {#if data.failed}
    <p class="rounded-md bg-danger/10 px-3 py-2 text-danger">Could not load links. Refresh to try again.</p>
  {/if}

  <!-- table -->
  <div>
    <div class="{grid} h-8 border-b border-border px-3 text-[11.5px] font-medium text-faint">
      <span>Link</span>
      <span class="hidden lg:block">Activity</span>
      <span class="hidden lg:block">Expires</span>
      <span class="hidden md:block">Status</span>
      <span></span>
    </div>

    {#if !links.length && !data.failed}
      <div class="py-16 text-center">
        <div class="mx-auto mb-3 grid h-11 w-11 place-items-center rounded-xl border border-border bg-surface text-faint"><Icon name="link" size={22} /></div>
        <div class="text-sm font-medium text-text">No links yet</div>
        <p class="mt-1 text-muted">Start from <a href="/links/new" class="text-accent-hover hover:underline">New link</a>.</p>
      </div>
    {:else if !shown.length}
      <p class="py-14 text-center text-faint">No links match “{q}”.</p>
    {/if}

    {#each groups as g (g.name)}
      {#if g.name}
        <div class="flex h-10 items-end gap-2 border-b border-border px-3 pb-2 font-semibold text-text">
          <span class="text-accent"><Icon name="project" /></span>{g.name}<span class="font-normal text-faint">{g.items.length} link{g.items.length === 1 ? "" : "s"}</span>
        </div>
      {/if}
      {#each g.items as l (l.id)}
        {@const st = linkStatus(l)}
        {@const live = l.activity.live[0]}
        {@const prog = live ? liveProgress(live) : null}
        {@const path = linkPath(l)}
        {@const isOpen = open.has(l.id)}
        <div class="border-b border-border {isOpen ? 'bg-surface/60' : ''} {l.state !== 'ok' ? 'opacity-80' : ''}">
          <div
            class="{grid} min-h-[60px] cursor-pointer px-3 py-2 hover:bg-white/[0.02]"
            role="button"
            tabindex="0"
            aria-expanded={isOpen}
            onclick={() => toggle(l)}
            onkeydown={(e) => {
              if ((e.key === "Enter" || e.key === " ") && e.target === e.currentTarget) {
                e.preventDefault();
                toggle(l);
              }
            }}
          >
            <div class="flex min-w-0 items-center gap-3">
              <div class="grid h-[30px] w-[30px] shrink-0 place-items-center rounded-md border border-border bg-surface-2 text-body" title={kindNoun(l.kind)}>
                <Icon name={l.kind === "upload" ? "receive" : "send"} />
              </div>
              <div class="min-w-0">
                <div class="flex min-w-0 items-center gap-1.5 font-medium text-text">
                  <span class="truncate">{l.name}</span>
                  {#if l.password_protected}<span class="text-faint" title="Password"><Icon name="lock" size={13} /></span>{/if}
                  {#if l.verify_email}<span class="text-faint" title="Email code"><Icon name="mail" size={13} /></span>{/if}
                  {#if l.kind === "upload" && l.folder?.sync}<span class="text-faint" title="Synced with FioTransfer"><Icon name="sync" size={13} /></span>{/if}
                </div>
                <div class="truncate text-xs text-faint">
                  {#each path as p, i (i)}{#if i > 0}<span class="px-1">/</span>{/if}<span class={i === path.length - 1 ? "text-muted" : ""}>{p}</span>{/each}
                  {#if l.kind === "upload" && l.subfolder_template}<span class="px-1">/</span><span class="font-mono text-[11px] text-muted">{l.subfolder_template}</span>{/if}
                  {#if l.kind === "download"}{#if path.length}<span class="px-1">·</span>{/if}{sourceSummary(l.source)}{/if}
                </div>
                <div class="mt-1 text-xs text-muted lg:hidden">
                  {#if live}{live.who ?? "Someone"} uploading{prog?.pct != null ? ` · ${Math.floor(prog.pct)}%` : ""}{:else}{st.text} · expires {expiresText(l).toLowerCase()}{/if}
                </div>
              </div>
            </div>

            <div class="hidden min-w-0 lg:block">
              {#if live}
                <div class="grid gap-1.5">
                  <div class="flex justify-between gap-2 text-xs whitespace-nowrap">
                    <b class="truncate font-medium text-text">{live.who ?? "Someone"}</b>
                    <span class="tnum text-muted">{formatBytes(live.uploaded_bytes ?? 0)}{live.total_bytes ? ` / ${formatBytes(live.total_bytes)}` : ""} · {fmtSpeed(live.avg_bps)}</span>
                  </div>
                  <div class="relative h-1 overflow-hidden rounded-full bg-surface-3">
                    <i class="absolute inset-y-0 left-0 rounded-full bg-accent transition-[width] duration-500" style="width:{prog?.pct ?? 8}%"></i>
                  </div>
                </div>
              {:else if l.kind === "upload"}
                {#if l.activity.count}
                  <span class="text-body">{l.activity.count} upload{l.activity.count === 1 ? "" : "s"} · {formatBytes(l.activity.bytes ?? 0)}</span>
                  <small class="block truncate text-xs text-faint">Last {timeAgo(l.activity.last_at)}{l.activity.last_who ? `, ${l.activity.last_who}` : ""}</small>
                {:else}
                  <span class="text-muted">Waiting for first upload</span>
                {/if}
              {:else}
                <span class="text-body">{l.activity.count} download{l.activity.count === 1 ? "" : "s"}{l.max_downloads ? ` of ${l.max_downloads}` : ""}</span>
                {#if l.activity.last_at}<small class="block truncate text-xs text-faint">Last {timeAgo(l.activity.last_at)}{l.activity.last_who ? `, ${l.activity.last_who}` : ""}</small>{/if}
              {/if}
            </div>

            <div class="tnum hidden whitespace-nowrap lg:block {l.state === 'ok' ? 'text-body' : 'text-faint'}">{expiresText(l)}</div>

            <div class="hidden min-w-0 md:block">
              <span class="inline-flex max-w-full items-center gap-2 text-[12.5px] whitespace-nowrap {statusTone[st.tone]}">
                {#if st.tone === "live"}<span class="live-dot"></span>{:else}<span class="h-[7px] w-[7px] shrink-0 rounded-full {dotTone[st.tone]}"></span>{/if}
                <span class="truncate">{st.text}</span>
              </span>
            </div>

            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="flex items-center justify-end gap-0.5" onclick={(e) => e.stopPropagation()}>
              {#if l.state === "expired"}
                <button type="button" class="h-7 rounded-md border border-border px-2.5 text-xs font-medium text-text hover:bg-surface-3" onclick={() => extend(l)}>Extend</button>
              {/if}
              {#if l.state === "revoked"}
                <button type="button" class="h-7 rounded-md border border-border px-2.5 text-xs font-medium text-text hover:bg-surface-3" onclick={() => restore(l)}>Restore</button>
              {:else}
                <a href={l.public_url} target="_blank" rel="noopener" class="hidden h-7 w-7 place-items-center rounded-md text-faint hover:bg-surface-3 hover:text-text sm:grid" title="Open client page" aria-label="Open client page"><Icon name="open" /></a>
                <button type="button" class="grid h-7 w-7 place-items-center rounded-md text-body hover:bg-surface-3 hover:text-text" title="Copy link" aria-label="Copy link" onclick={() => copyWithToast(l.public_url)}><Icon name="copy" /></button>
              {/if}
              {#if l.state === "ok"}
                <ConfirmButton label="Revoke link" confirmLabel="Revoke" icon="revoke" onconfirm={() => revoke(l)} />
              {/if}
              <ConfirmButton label="Delete link" confirmLabel="Delete forever" icon="trash" onconfirm={() => remove(l)} />
              <a href={editHref(l)} class="grid h-7 w-7 place-items-center rounded-md text-faint hover:bg-surface-3 hover:text-text" title="Edit" aria-label="Edit {l.name}"><Icon name="pencil" /></a>
            </div>
          </div>

          {#if isOpen}
            {@const d = details[l.id]}
            <div class="grid gap-5 px-3 pb-4 md:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)] md:pl-[54px]">
              <div class="grid content-start gap-x-4 gap-y-1.5 text-[12.5px] sm:grid-cols-[100px_minmax(0,1fr)]">
                <span class="text-faint">Client page</span>
                <button type="button" class="min-w-0 truncate text-left font-mono text-[11.5px] text-text hover:text-accent-hover" title="Copy link" onclick={() => copyWithToast(l.public_url)}>{l.public_url.replace(/^https?:\/\//, "")}</button>
                <span class="text-faint">{l.kind === "upload" ? "Lands in" : "Sends"}</span>
                <span class="min-w-0 break-words text-body">{[path.join(" / "), l.kind === "download" ? sourceSummary(l.source) : ""].filter(Boolean).join(" · ")}</span>
                {#if l.kind === "upload"}
                  {@const sync = l.folder?.sync}
                  <span class="text-faint">FioTransfer</span>
                  <span class="min-w-0 break-words text-body">
                    {#if sync}
                      {[sync.drive && "Imports", sync.exports && "Exports"].filter(Boolean).join(" and ")}{#if l.turbosort}<span class="text-faint"> · in </span><span class="font-mono text-[11.5px]">{l.turbosort}</span>{/if}
                    {:else if l.turbosort}
                      Mirrored both ways<span class="text-faint"> · as </span><span class="font-mono text-[11.5px]">{l.turbosort}</span>
                    {:else}
                      Not synced
                    {/if}
                  </span>
                  {#if l.return_link}
                    {@const ret = l.return_link}
                    <span class="text-faint">Clients download</span>
                    <button type="button" class="min-w-0 truncate text-left font-mono text-[11.5px] text-text hover:text-accent-hover" title="Copy link" onclick={() => copyWithToast(ret.public_url)}>{ret.public_url.replace(/^https?:\/\//, "")}</button>
                  {/if}
                {:else if l.return_for}
                  {@const owner = links.find((x) => x.id === l.return_for)}
                  <span class="text-faint">Follows</span>
                  <a href="/upload-links/{l.return_for}" class="min-w-0 truncate text-body hover:text-accent-hover">{owner?.name ?? "its receive link"}<span class="text-faint"> · send folder</span></a>
                {/if}
                <span class="text-faint">Protection</span>
                <span class="text-body">{[l.verify_email && "Email code", l.password_protected && "Password"].filter(Boolean).join(" · ") || "None"}</span>
                <span class="text-faint">Expires</span>
                <span class="text-body">{l.expires_at ? whenFull(l.expires_at) : "Never"}</span>
                <span class="text-faint">Created</span>
                <span class="text-body">{whenFull(l.created_at)}</span>
                <div class="col-span-full mt-2 flex flex-wrap gap-1.5">
                  <a href={editHref(l)} class="inline-flex h-7 items-center gap-1.5 rounded-md border border-border px-2.5 text-xs font-medium text-text hover:bg-surface-3"><Icon name="pencil" size={13} />Edit</a>
                  <ConfirmButton label="Delete" confirmLabel="Delete forever" onconfirm={() => remove(l)} />
                </div>
              </div>

              <div class="self-start overflow-hidden rounded-lg border border-border bg-surface">
                <div class="border-b border-border px-3 py-2 text-[11.5px] font-medium text-faint">{l.kind === "upload" ? "Recent uploads" : "Download log"}</div>
                {#if !d}
                  <div class="px-3 py-3 text-faint">Loading…</div>
                {:else if d.error}
                  <div class="px-3 py-3 text-danger">{d.error}</div>
                {:else if l.kind === "upload"}
                  {#each d.sessions ?? [] as s (s.id)}
                    <div class="grid h-8 grid-cols-[minmax(0,1fr)_56px_150px_96px] items-center gap-2.5 border-b border-border px-3 text-xs last:border-b-0">
                      <span class="truncate text-text">{who(s.uploader_name, s.uploader_email)}{#if s.uploader_project}<span class="text-faint"> · {s.uploader_project}</span>{/if}</span>
                      <span class="tnum truncate text-muted">{s.files} file{s.files === 1 ? "" : "s"}</span>
                      <span class="tnum truncate text-muted">{s.status === "in_progress" && s.uploaded_bytes != null ? `${formatBytes(s.uploaded_bytes)} of ` : ""}{s.total_bytes != null ? formatBytes(s.total_bytes) : "—"}</span>
                      <span class="truncate text-right {s.status === 'in_progress' ? 'text-accent-hover' : s.status === 'abandoned' ? 'text-warning' : 'text-muted'}">
                        {s.status === "in_progress" ? "Uploading now" : s.status === "abandoned" ? "Didn't finish" : timeAgo(s.completed_at ?? s.last_activity_at)}
                      </span>
                    </div>
                  {:else}
                    <div class="px-3 py-3 text-faint">No uploads yet.</div>
                  {/each}
                {:else}
                  {#each d.events ?? [] as e, i (i)}
                    <div class="grid h-8 grid-cols-[minmax(0,1fr)_minmax(0,1fr)_80px] items-center gap-2.5 border-b border-border px-3 text-xs last:border-b-0">
                      <span class="truncate text-text">{e.viewer_name || e.viewer_email || e.ip || "Someone"}</span>
                      <span class="truncate text-muted">{e.file_name ?? e.frameio_file_id}</span>
                      <span class="truncate text-right text-muted" title={whenFull(e.started_at)}>{timeAgo(e.started_at)}</span>
                    </div>
                  {:else}
                    <div class="px-3 py-3 text-faint">No downloads yet.</div>
                  {/each}
                {/if}
              </div>
            </div>
          {/if}
        </div>
      {/each}
    {/each}
  </div>
</div>
