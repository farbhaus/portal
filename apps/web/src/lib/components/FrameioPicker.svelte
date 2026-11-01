<script lang="ts" module>
  import type { DownloadSource, FolderRef, PathSegment } from "$lib/frameio";

  export type SourcePick = {
    source: DownloadSource;
    label: string;
    path: PathSegment[];
    count: number | null;
    bytes: number | null;
  };
</script>

<script lang="ts">
  // The one Frame.io picker: a modal that walks accounts → workspaces → projects → folders, with
  // search across the whole account, recent folders, keyboard control and New folder / New project.
  //  - mode "folder": choose a folder (upload link destinations, sync rule sources).
  //  - mode "source": choose files or a whole folder to send (download links).
  // Accounts and workspaces are skipped when there's only one of them.
  import { tick, untrack } from "svelte";
  import Icon from "./Icon.svelte";
  import {
    createFolder,
    createProject,
    crumbNames,
    folderKey,
    listAccounts,
    listFiles,
    listFolders,
    listProjects,
    listWorkspaces,
    resolvePath,
    search,
    type FileItem,
    type SearchHit,
  } from "$lib/frameio";
  import { formatBytes } from "$lib/format";

  type Kind = "root" | "account" | "workspace" | "project" | "folder";
  type Crumb = { kind: Kind; id: string; name: string; folder_id?: string };
  type Row =
    | { kind: "account" | "workspace" | "project" | "folder"; id: string; name: string; folder_id?: string }
    | { kind: "file"; id: string; name: string; size: number | null }
    | { kind: "recent"; id: string; name: string; where: string; ref: FolderRef }
    | { kind: "hit"; id: string; name: string; where: string; hit: SearchHit };

  let {
    open = $bindable(false),
    mode = "folder",
    title = "Choose a folder",
    start = null,
    startAtProjects = false,
    recents = [],
    chooseVerb = "Choose",
    onchoose,
    onsource,
  }: {
    open?: boolean;
    mode?: "folder" | "source";
    title?: string;
    // Open inside this folder (or, with startAtProjects, at its workspace's project list).
    start?: FolderRef | null;
    startAtProjects?: boolean;
    recents?: FolderRef[];
    chooseVerb?: string;
    onchoose?: (folder: FolderRef) => void;
    onsource?: (pick: SourcePick) => void;
  } = $props();

  const ROOT: Crumb = { kind: "root", id: "", name: "Frame.io" };

  let stack = $state<Crumb[]>([ROOT]);
  let items = $state<Row[] | null>(null);
  let files = $state<FileItem[]>([]);
  let error = $state<string | null>(null);
  let q = $state("");
  let hits = $state<SearchHit[] | null>(null);
  let hitsError = $state<string | null>(null);
  let sel = $state(0);
  let kbd = $state(false);
  let selected = $state(new Map<string, FileItem>());
  let recursive = $state(false);
  let newOpen = $state(false);
  let newName = $state("");
  let newProjectOpen = $state(false);
  let newProjectName = $state("");
  let creating = $state(false);
  let defaultAccount = $state("");
  let input = $state<HTMLInputElement | null>(null);
  // The name field of whichever inline form (New folder / New project) is open.
  let formInput = $state<HTMLInputElement | null>(null);
  let list = $state<HTMLDivElement | null>(null);
  let seq = 0;
  let searchSeq = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  const top = $derived(stack[stack.length - 1]);
  const accountId = $derived(stack.find((c) => c.kind === "account")?.id ?? "");
  const workspaceId = $derived(stack.find((c) => c.kind === "workspace")?.id ?? "");
  const inFolder = $derived(top.kind === "project" || top.kind === "folder");
  const folderId = $derived(top.kind === "project" ? top.folder_id : top.kind === "folder" ? top.id : undefined);
  const searchAccount = $derived(accountId || start?.account_id || recents[0]?.account_id || defaultAccount);

  $effect(() => {
    if (open) untrack(() => void reset());
  });

  async function reset() {
    q = "";
    hits = null;
    hitsError = null;
    sel = 0;
    kbd = false;
    selected = new Map();
    recursive = false;
    newOpen = false;
    newName = "";
    newProjectOpen = false;
    newProjectName = "";
    error = null;
    if (start?.account_id && start.folder_id) await openAt(start, startAtProjects);
    else await go([ROOT], true);
    await tick();
    input?.focus();
  }

  const crumbOf = (r: Row): Crumb | null =>
    r.kind === "account" || r.kind === "workspace" || r.kind === "project" || r.kind === "folder"
      ? { kind: r.kind, id: r.id, name: r.name, folder_id: r.folder_id }
      : null;

  async function go(next: Crumb[], autoSkip = false) {
    const my = ++seq;
    stack = next;
    items = null;
    files = [];
    error = null;
    newOpen = false;
    newProjectOpen = false;
    if (list) list.scrollTop = 0;
    const t = next[next.length - 1];
    const acct = next.find((c) => c.kind === "account")?.id ?? "";
    try {
      let rows: Row[];
      if (t.kind === "root") {
        const accounts = await listAccounts();
        if (accounts[0]) defaultAccount = accounts[0].id;
        rows = accounts.map((a) => ({ kind: "account", id: a.id, name: a.name }));
      } else if (t.kind === "account") {
        rows = (await listWorkspaces(t.id)).map((w) => ({ kind: "workspace", id: w.id, name: w.name }));
      } else if (t.kind === "workspace") {
        rows = (await listProjects(acct, t.id))
          .filter((p) => p.root_folder_id)
          .sort((a, b) => a.name.localeCompare(b.name))
          .map((p) => ({ kind: "project", id: p.id, name: p.name, folder_id: p.root_folder_id ?? undefined }));
      } else {
        const fid = t.kind === "project" ? t.folder_id! : t.id;
        // Folder mode lists files too, dimmed, so you can tell you're in the right place.
        const [folders, fileList] = await Promise.all([
          listFolders(acct, fid),
          listFiles(acct, fid).catch(() => [] as FileItem[]),
        ]);
        if (my !== seq) return;
        files = fileList;
        rows = folders.map((f) => ({ kind: "folder", id: f.id, name: f.name }));
      }
      if (my !== seq) return;
      if (autoSkip && rows.length === 1 && (t.kind === "root" || t.kind === "account")) {
        return go([...next, crumbOf(rows[0])!], true);
      }
      items = rows;
      // Keep typing in search after clicking around (mouse users; phones keep their keyboard down).
      if (open && window.matchMedia?.("(pointer: fine)").matches) input?.focus();
    } catch (e) {
      if (my !== seq) return;
      error = e instanceof Error ? e.message : "Could not load from Frame.io";
      items = [];
    }
  }

  /** Navigate to a folder we know by id (a recent, a search hit, the field's current value). */
  async function openAt(f: FolderRef, atProjects = false) {
    const my = ++seq;
    items = null;
    error = null;
    try {
      let { workspace_id: ws, path, project_id: projectId, project_name: projectName } = f;
      if (!ws || !path?.length || !projectId) {
        const r = await resolvePath(f.account_id, f.folder_id);
        ({ workspace_id: ws, path, project_id: projectId, project_name: projectName } = r);
      }
      const accounts = await listAccounts();
      const workspaces = ws ? await listWorkspaces(f.account_id) : [];
      if (my !== seq) return;
      if (accounts[0]) defaultAccount = accounts[0].id;
      const next: Crumb[] = [ROOT];
      next.push({ kind: "account", id: f.account_id, name: accounts.find((a) => a.id === f.account_id)?.name ?? "Account" });
      if (ws) next.push({ kind: "workspace", id: ws, name: workspaces.find((w) => w.id === ws)?.name ?? "Workspace" });
      if (atProjects) return go(next);
      const segs = path ?? [];
      const projectLabel = (projectName ?? segs[0]?.name ?? "Project").replace(/ \(root\)$/, "");
      next.push({ kind: "project", id: projectId ?? "", name: projectLabel, folder_id: segs[0]?.id });
      for (const s of segs.slice(1)) next.push({ kind: "folder", id: s.id, name: s.name });
      await go(next);
    } catch (e) {
      if (my !== seq) return;
      error = e instanceof Error ? e.message : "That folder is no longer on Frame.io";
      await go([ROOT], true);
    }
  }

  // ── search ──────────────────────────────────────────────────────────────
  function onQuery() {
    sel = 0;
    hits = null;
    hitsError = null;
    clearTimeout(searchTimer);
    const s = q.trim();
    if (s.length < 2 || !searchAccount) return;
    const my = ++searchSeq;
    searchTimer = setTimeout(async () => {
      try {
        const res = await search(searchAccount, s);
        if (my === searchSeq) hits = res;
      } catch (e) {
        if (my === searchSeq) hitsError = e instanceof Error ? e.message : "Search failed";
      }
    }, 300);
  }

  const view = $derived.by(() => {
    const s = q.trim().toLowerCase();
    const match = (name: string) => !s || name.toLowerCase().includes(s);
    const here: Row[] = (items ?? []).filter((r) => match(r.name));
    const fileRows: Row[] = mode === "source" ? files.filter((f) => match(f.name)).map((f) => ({ kind: "file", id: f.id, name: f.name, size: f.file_size })) : [];
    const ids = new Set([...here.map((r) => r.id), ...here.map((r) => ("folder_id" in r ? r.folder_id : undefined))]);
    const there: Row[] =
      s.length >= 2 && hits
        ? hits
            .filter((h) => !ids.has(h.id) && !(h.root_folder_id && ids.has(h.root_folder_id)))
            .map((h) => ({ kind: "hit", id: h.id, name: h.name, where: h.kind === "project" ? "Project" : (h.where ?? ""), hit: h }))
        : [];
    const atTop = top.kind === "root" || top.kind === "account" || top.kind === "workspace";
    const rec: Row[] =
      !s && atTop
        ? recents.slice(0, 5).map((r) => ({
            kind: "recent",
            id: folderKey(r),
            name: crumbNames(r).at(-1) ?? r.folder_id,
            where: crumbNames(r).slice(0, -1).join(" / "),
            ref: r,
          }))
        : [];
    // In folder mode files are context only: shown, not selectable.
    const contextFiles = mode === "folder" ? files.filter((f) => match(f.name)) : [];
    return { rec, here, fileRows, there, contextFiles };
  });
  const rows = $derived([...view.rec, ...view.here, ...view.fileRows, ...view.there]);
  const showSel = $derived(!!q.trim() || kbd);

  function highlight(name: string): { a: string; b: string; c: string } | null {
    const s = q.trim();
    const i = s ? name.toLowerCase().indexOf(s.toLowerCase()) : -1;
    return i < 0 ? null : { a: name.slice(0, i), b: name.slice(i, i + s.length), c: name.slice(i + s.length) };
  }

  async function openRow(r: Row | undefined) {
    if (!r) return;
    if (r.kind === "file") {
      toggleFile(r.id);
      return;
    }
    q = "";
    hits = null;
    sel = 0;
    kbd = false;
    if (r.kind === "recent") return openAt(r.ref);
    if (r.kind === "hit") {
      const fid = r.hit.kind === "project" ? r.hit.root_folder_id : r.hit.id;
      if (fid) return openAt({ account_id: searchAccount, folder_id: fid });
      return;
    }
    const c = crumbOf(r);
    if (c) await go([...stack, c], true);
  }

  function toggleFile(id: string) {
    const f = files.find((x) => x.id === id);
    if (!f) return;
    const next = new Map(selected);
    if (next.has(id)) next.delete(id);
    else next.set(id, f);
    selected = next;
  }

  function keep(i: number) {
    sel = Math.max(0, Math.min(rows.length - 1, i));
    queueMicrotask(() => {
      const el = list?.querySelector<HTMLElement>(`[data-i="${sel}"]`);
      if (!el || !list) return;
      const a = el.offsetTop;
      const b = a + el.offsetHeight;
      if (a < list.scrollTop) list.scrollTop = a;
      else if (b > list.scrollTop + list.clientHeight) list.scrollTop = b - list.clientHeight;
    });
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (showSel) keep(sel + (e.key === "ArrowDown" ? 1 : -1));
      kbd = true;
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (showSel) void openRow(rows[sel]);
      else if (mode === "folder" && inFolder) choose();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (q) {
        q = "";
        hits = null;
      } else open = false;
    } else if (e.key === "Backspace" && !q && stack.length > 1) {
      e.preventDefault();
      void go(stack.slice(0, -1));
    }
  }

  // ── results ─────────────────────────────────────────────────────────────
  function currentPath(): PathSegment[] {
    const proj = stack.find((c) => c.kind === "project");
    if (!proj) return [];
    return [{ id: proj.folder_id ?? proj.id, name: proj.name }, ...stack.filter((c) => c.kind === "folder").map((c) => ({ id: c.id, name: c.name }))];
  }

  function currentRef(): FolderRef {
    const path = currentPath();
    const proj = stack.find((c) => c.kind === "project");
    const last = path[path.length - 1];
    return {
      type: "frameio",
      account_id: accountId,
      workspace_id: stack.find((c) => c.kind === "workspace")?.id ?? null,
      project_id: proj?.id ?? null,
      project_name: proj?.name ?? null,
      folder_id: last.id,
      folder_name: last.name,
      path,
    };
  }

  function choose() {
    if (!inFolder) return;
    onchoose?.(currentRef());
    open = false;
  }

  const selBytes = $derived([...selected.values()].reduce((n, f) => n + (f.file_size ?? 0), 0));

  function sendSelection() {
    const path = currentPath();
    const picked = [...selected.values()];
    if (!picked.length) return;
    const names = picked.map((f) => f.name);
    const source: DownloadSource =
      picked.length === 1
        ? { type: "file", account_id: accountId, file_id: picked[0].id, path, label: names[0], names, bytes: selBytes }
        : { type: "selection", account_id: accountId, file_ids: picked.map((f) => f.id), path, label: `${picked.length} files`, names: names.slice(0, 50), bytes: selBytes };
    onsource?.({ source, label: source.label ?? "", path, count: picked.length, bytes: selBytes || null });
    open = false;
  }

  function sendFolder() {
    if (!inFolder || !folderId) return;
    const path = currentPath();
    const source: DownloadSource = { type: "folder", account_id: accountId, folder_id: folderId, recursive, path, label: top.name };
    onsource?.({ source, label: top.name, path, count: null, bytes: null });
    open = false;
  }

  async function makeFolder() {
    const name = newName.trim();
    if (!name || !folderId || creating) return;
    creating = true;
    error = null;
    try {
      const f = await createFolder(accountId, folderId, name);
      newName = "";
      newOpen = false;
      // In folder mode the new folder is the answer: hand it back so the click ends with it chosen.
      if (mode === "folder") return chooseFolder(f.id, f.name);
      await go([...stack, { kind: "folder", id: f.id, name: f.name }]);
    } catch (e) {
      error = e instanceof Error ? e.message : "Could not create the folder";
    } finally {
      creating = false;
    }
  }

  /** Hand `onchoose` the folder we are in, one level deeper (a folder that was just made). */
  function chooseFolder(id: string, name: string) {
    const path = [...currentPath(), { id, name }];
    onchoose?.({ ...currentRef(), folder_id: id, folder_name: name, path });
    open = false;
  }

  async function makeProject() {
    const name = newProjectName.trim();
    if (!name || !accountId || !workspaceId || creating) return;
    creating = true;
    error = null;
    try {
      const p = await createProject(accountId, workspaceId, name);
      newProjectName = "";
      newProjectOpen = false;
      const ws = stack.findIndex((c) => c.kind === "workspace");
      const base = ws >= 0 ? stack.slice(0, ws + 1) : [...stack];
      const root = p.root_folder_id;
      if (root) {
        // Same footing as New folder: the new project's root folder is the answer.
        if (mode === "folder") {
          const path = [{ id: root, name: p.name }];
          onchoose?.({
            type: "frameio",
            account_id: accountId,
            workspace_id: workspaceId,
            project_id: p.id,
            project_name: p.name,
            folder_id: root,
            folder_name: p.name,
            path,
          });
          open = false;
          return;
        }
        return go([...base, { kind: "project", id: p.id, name: p.name, folder_id: root }]);
      }
      // Frame.io answered without a root folder; show the project in the list and say why we stopped.
      await go(base);
      error = `“${p.name}” was created but has no root folder to receive into yet`;
    } catch (e) {
      error = e instanceof Error ? e.message : "Could not create the project";
    } finally {
      creating = false;
    }
  }

  const iconFor = (r: Row) =>
    r.kind === "file" ? "file" : r.kind === "project" || (r.kind === "hit" && r.hit.kind === "project") ? "project" : r.kind === "account" || r.kind === "workspace" ? "team" : "folder";
</script>

{#if open}
  <div
    class="fixed inset-0 z-50 grid place-items-center bg-black/70 p-4"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) open = false;
    }}
  >
    <div
      class="flex h-[min(560px,86vh)] w-full max-w-[580px] flex-col overflow-hidden rounded-xl border border-border-strong bg-surface text-[13px] shadow-2xl"
      role="dialog"
      aria-modal="true"
      aria-label={title}
    >
      <div class="flex items-center gap-2 py-3 pr-2.5 pl-4">
        <div class="min-w-0 flex-1 truncate text-sm font-semibold text-text">{title}</div>
        <button type="button" class="grid h-7 w-7 place-items-center rounded-md text-faint hover:bg-surface-3 hover:text-text" aria-label="Close" onclick={() => (open = false)}>
          <Icon name="x" />
        </button>
      </div>

      <label class="relative mx-3 mb-2 flex items-center">
        <span class="pointer-events-none absolute left-2.5 text-faint"><Icon name="search" /></span>
        <input
          bind:this={input}
          bind:value={q}
          oninput={onQuery}
          onkeydown={onKey}
          type="text"
          placeholder={mode === "source" ? "Search folders and files" : "Search folders"}
          aria-label="Search"
          autocomplete="off"
          spellcheck="false"
          class="h-8 w-full rounded-md border border-border bg-bg pr-2 pl-8 text-[13px] outline-none focus:border-accent focus:ring-3 focus:ring-accent-soft"
        />
      </label>

      <div class="flex flex-wrap items-center gap-px border-b border-border px-3 pb-2 text-[12.5px]">
        {#each stack as c, i (i)}
          {#if i > 0}<span class="px-0.5 text-faint">/</span>{/if}
          <button
            type="button"
            class="max-w-48 truncate rounded px-1.5 py-0.5 hover:bg-surface-3 hover:text-text {i === stack.length - 1 ? 'font-medium text-text' : 'text-muted'}"
            onclick={() => void go(stack.slice(0, i + 1))}
          >{c.name}</button>
        {/each}
      </div>

      <div bind:this={list} class="relative min-h-0 flex-1 overflow-y-auto p-1.5" role="listbox" aria-label="Folders">
        {#if error}
          <p class="mx-2 my-2 rounded-md bg-danger/10 px-3 py-2 text-danger">
            {error}{#if /not connected/i.test(error)}. <a href="/settings" class="font-medium underline">Connect Frame.io in Settings</a>{/if}
          </p>
        {/if}
        {#if items === null}
          <div class="grid place-items-center py-14 text-faint">
            <span class="h-3.5 w-3.5 animate-spin rounded-full border-[1.5px] border-border-strong border-t-muted"></span>
          </div>
        {:else}
          {#snippet item(r: Row, i: number)}
            {@const hl = highlight(r.name)}
            {@const picked = r.kind === "file" && selected.has(r.id)}
            <div
              data-i={i}
              role="option"
              tabindex="-1"
              aria-selected={showSel && i === sel}
              class="flex h-[34px] cursor-pointer items-center gap-2.5 rounded-md px-2.5 {showSel && i === sel ? 'bg-surface-3 text-text' : 'text-body hover:bg-surface-2 hover:text-text'}"
              onclick={() => void openRow(r)}
              onkeydown={() => {}}
            >
              {#if r.kind === "file"}
                <span class="grid h-4 w-4 place-items-center rounded border {picked ? 'border-accent bg-accent text-on-accent' : 'border-border-strong'}">
                  {#if picked}<Icon name="check" size={12} />{/if}
                </span>
              {/if}
              <span class={r.kind === "project" || (r.kind === "hit" && r.hit.kind === "project") ? "text-accent" : "text-faint"}><Icon name={iconFor(r)} /></span>
              <span class="min-w-0 flex-1 truncate">
                {#if hl}{hl.a}<mark class="bg-transparent font-semibold text-accent-hover">{hl.b}</mark>{hl.c}{:else}{r.name}{/if}
              </span>
              {#if r.kind === "recent" || r.kind === "hit"}
                <span class="max-w-[46%] truncate text-xs text-faint">{r.where}</span>
              {/if}
              {#if r.kind === "file"}
                <span class="tnum text-xs text-faint">{r.size != null ? formatBytes(r.size) : ""}</span>
              {:else}
                <span class="text-faint"><Icon name="chevron-right" size={14} /></span>
              {/if}
            </div>
          {/snippet}

          {#if view.rec.length}
            <div class="px-2.5 pt-2.5 pb-1 text-[11.5px] font-medium text-faint">Recent</div>
            {#each view.rec as r, i (r.id)}{@render item(r, i)}{/each}
            {#if view.here.length}
              <div class="px-2.5 pt-3 pb-1 text-[11.5px] font-medium text-faint">
                {top.kind === "workspace" ? "Projects" : top.kind === "account" ? "Workspaces" : "Accounts"}
              </div>
            {/if}
          {/if}
          {#each view.here as r, i (r.kind + r.id)}{@render item(r, view.rec.length + i)}{/each}
          {#each view.fileRows as r, i (r.id)}{@render item(r, view.rec.length + view.here.length + i)}{/each}
          {#if view.contextFiles.length}
            <div class="px-2.5 pt-2.5 pb-1 text-[11.5px] font-medium text-faint">{view.contextFiles.length} file{view.contextFiles.length === 1 ? "" : "s"} in here</div>
            {#each view.contextFiles.slice(0, 40) as f (f.id)}
              <div class="flex h-[30px] items-center gap-2.5 px-2.5 text-faint">
                <Icon name="file" />
                <span class="min-w-0 flex-1 truncate">{f.name}</span>
                <span class="tnum text-xs">{f.file_size != null ? formatBytes(f.file_size) : ""}</span>
              </div>
            {/each}
            {#if view.contextFiles.length > 40}<div class="px-2.5 py-1 text-xs text-faint">and {view.contextFiles.length - 40} more</div>{/if}
          {/if}

          {#if !q.trim() && !error && !view.here.length && !view.fileRows.length && !view.contextFiles.length}
            <p class="py-12 text-center text-faint">{inFolder ? (mode === "source" ? "This folder is empty" : "No folders in here yet") : "Nothing here"}</p>
          {/if}

          {#if q.trim().length >= 2}
            {#if hitsError}
              <p class="px-2.5 py-2 text-danger">{hitsError}</p>
            {:else if hits === null}
              <div class="flex items-center gap-2 px-2.5 py-3 text-faint">
                <span class="h-3 w-3 animate-spin rounded-full border-[1.5px] border-border-strong border-t-muted"></span>
                Searching every project…
              </div>
            {:else if view.there.length}
              <div class="px-2.5 pt-3 pb-1 text-[11.5px] font-medium text-faint">All projects</div>
              {#each view.there as r, i (r.id)}{@render item(r, view.rec.length + view.here.length + view.fileRows.length + i)}{/each}
            {/if}
          {/if}
          {#if q.trim() && !rows.length && (q.trim().length < 2 || hits !== null)}
            <p class="py-12 text-center text-faint">{q.trim().length < 2 ? "Nothing here matches. Keep typing to search every project." : "No folders match"}</p>
          {/if}
        {/if}
      </div>

      <div class="flex flex-wrap items-center gap-2 border-t border-border px-3 py-2.5">
        {#if mode === "folder" || selected.size === 0}
          {#if newOpen}
            <input
              bind:this={formInput}
              bind:value={newName}
              placeholder="Folder name"
              aria-label="New folder name"
              class="h-8 w-40 rounded-md border border-border bg-bg px-2.5 outline-none focus:border-accent"
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void makeFolder();
                }
                if (e.key === "Escape") {
                  e.stopPropagation();
                  newOpen = false;
                }
              }}
            />
            <button type="button" class="h-8 rounded-md px-3 font-medium text-text hover:bg-surface-3 disabled:opacity-40" disabled={!newName.trim() || creating} onclick={makeFolder}>
              {creating ? "Creating…" : "Create"}
            </button>
          {:else if newProjectOpen}
            <input
              bind:this={formInput}
              bind:value={newProjectName}
              placeholder="Project name"
              aria-label="New project name"
              class="h-8 w-40 rounded-md border border-border bg-bg px-2.5 outline-none focus:border-accent"
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void makeProject();
                }
                if (e.key === "Escape") {
                  e.stopPropagation();
                  newProjectOpen = false;
                }
              }}
            />
            <button type="button" class="h-8 rounded-md px-3 font-medium text-text hover:bg-surface-3 disabled:opacity-40" disabled={!newProjectName.trim() || creating || !workspaceId} onclick={makeProject}>
              {creating ? "Creating…" : "Create"}
            </button>
          {:else}
            <button type="button" class="h-8 rounded-md px-3 font-medium text-muted hover:bg-surface-3 hover:text-text disabled:opacity-40" disabled={!inFolder} onclick={async () => { newProjectOpen = false; newOpen = true; await tick(); formInput?.focus(); }}>
              New folder
            </button>
            <button
              type="button"
              class="h-8 rounded-md px-3 font-medium text-muted hover:bg-surface-3 hover:text-text disabled:opacity-40"
              disabled={!accountId || !workspaceId}
              title={workspaceId ? "Create a Frame.io project and receive into its root folder" : "Open a workspace first"}
              onclick={async () => { newOpen = false; newProjectOpen = true; await tick(); formInput?.focus(); }}
            >
              New project
            </button>
          {/if}
        {/if}
        <span class="flex-1"></span>
        <span class="hidden font-mono text-[11px] text-faint sm:inline">↑↓ ↵ esc</span>
        {#if mode === "folder"}
          <button type="button" class="h-8 rounded-md px-3 font-medium text-muted hover:bg-surface-3 hover:text-text" onclick={() => (open = false)}>Cancel</button>
          <button
            type="button"
            class="h-8 max-w-[260px] truncate rounded-md bg-accent px-3.5 font-medium text-on-accent hover:bg-accent-hover disabled:opacity-40"
            disabled={!inFolder}
            onclick={choose}
          >{inFolder ? `${chooseVerb} “${top.name}”` : chooseVerb}</button>
        {:else if selected.size > 0}
          <span class="tnum text-muted">{selected.size} file{selected.size === 1 ? "" : "s"}{selBytes ? ` · ${formatBytes(selBytes)}` : ""}</span>
          <button type="button" class="h-8 rounded-md px-3 font-medium text-muted hover:bg-surface-3 hover:text-text" onclick={() => (selected = new Map())}>Clear</button>
          <button type="button" class="h-8 rounded-md bg-accent px-3.5 font-medium text-on-accent hover:bg-accent-hover" onclick={sendSelection}>
            Send {selected.size === 1 ? "this file" : `${selected.size} files`}
          </button>
        {:else}
          <label class="flex items-center gap-2 text-muted {inFolder ? '' : 'opacity-40'}">
            <input type="checkbox" bind:checked={recursive} disabled={!inFolder} /> Include subfolders
          </label>
          <button type="button" class="h-8 max-w-[240px] truncate rounded-md bg-accent px-3.5 font-medium text-on-accent hover:bg-accent-hover disabled:opacity-40" disabled={!inFolder} onclick={sendFolder}>
            {inFolder ? `Send “${top.name}”` : "Send this folder"}
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}
