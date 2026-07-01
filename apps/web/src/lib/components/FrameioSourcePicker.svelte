<script lang="ts" module>
  // The Frame.io source a download link serves: a single file, a curated selection, or a folder.
  // Mirrors the type-tagged `source` JSONB the API validates (portal.downloads.links.validate_source).
  export type DownloadSource =
    | { type: "file"; account_id: string; file_id: string }
    | { type: "selection"; account_id: string; file_ids: string[] }
    | { type: "folder"; account_id: string; folder_id: string; recursive: boolean };

  // Deep-link the explorer straight into an existing folder source: `path` is the breadcrumb from
  // the top-most ancestor down to the shared folder ({id,name} each). accountId/workspaceId/projectId
  // pre-select the account + workspace + project selectors to match the shared folder.
  export type SourcePickerInit = {
    accountId: string;
    workspaceId?: string;
    projectId?: string;
    path: { id: string; name: string }[];
    recursive?: boolean;
  };
</script>

<script lang="ts">
  import { ProjectPicker } from "$lib/components";

  // Shared by the new-link page and the edit page's "change source" flow. `value` reflects the
  // currently-picked source (null until something is chosen); load errors bubble via `onerror` so
  // the host page can show them in its own banner.
  let {
    value = $bindable(null),
    initial = null,
    onerror,
  }: {
    value?: DownloadSource | null;
    initial?: SourcePickerInit | null;
    onerror?: (msg: string | null) => void;
  } = $props();

  type Item = { id: string; name: string };
  type Project = { id: string; name: string; root_folder_id: string | null };
  type FileItem = { id: string; name: string; file_size: number | null };

  let subfolders = $state<Item[]>([]);
  let files = $state<FileItem[]>([]);
  let accountId = $state("");
  let workspaceId = $state("");
  let projectId = $state("");
  let folderPath = $state<Item[]>([]);
  let selected = $state<Map<string, string>>(new Map());
  let folderShare = $state<Item | null>(null);
  let recursive = $state(false);
  let loadingFrameio = $state(false);
  let newFolderName = $state("");
  let creatingFolder = $state(false);

  const currentFolder = $derived(folderPath.at(-1) ?? null);
  const hasSource = $derived(folderShare !== null || selected.size > 0);

  function buildSource(): DownloadSource | null {
    if (folderShare) return { type: "folder", account_id: accountId, folder_id: folderShare.id, recursive };
    const ids = [...selected.keys()];
    if (ids.length === 1) return { type: "file", account_id: accountId, file_id: ids[0] };
    if (ids.length > 1) return { type: "selection", account_id: accountId, file_ids: ids };
    return null;
  }
  const source = $derived(buildSource());
  $effect(() => {
    value = source;
  });

  // When given an existing folder source, open the explorer already inside it: seed the breadcrumb,
  // mark it as the shared folder, and load its contents. Runs once.
  let seeded = false;
  $effect(() => {
    if (seeded || !initial || initial.path.length === 0) return;
    seeded = true;
    accountId = initial.accountId;
    projectId = initial.projectId ?? "";
    folderPath = initial.path.map((p) => ({ id: p.id, name: p.name }));
    folderShare = folderPath[folderPath.length - 1];
    recursive = initial.recursive ?? false;
    void loadFolder();
  });

  async function getJSON<T>(url: string): Promise<T> {
    const res = await fetch(url);
    if (!res.ok) throw new Error((await res.json().catch(() => ({}))).detail ?? `Failed (${res.status})`);
    return res.json();
  }
  async function loadFrameio<T>(fn: () => Promise<T>) {
    loadingFrameio = true;
    onerror?.(null);
    try {
      return await fn();
    } catch (e) {
      onerror?.(e instanceof Error ? e.message : "Failed to load from Frame.io");
    } finally {
      loadingFrameio = false;
    }
  }

  function onScopeReset() {
    projectId = "";
    subfolders = files = [];
    folderPath = [];
    clearSource();
  }
  async function onProjectSelect(proj: Project) {
    projectId = proj.id;
    subfolders = files = [];
    folderPath = [];
    clearSource();
    if (!proj.root_folder_id) return;
    folderPath = [{ id: proj.root_folder_id, name: `${proj.name} (root)` }];
    await loadFolder();
  }
  async function loadFolder() {
    if (!currentFolder) return;
    await loadFrameio(async () => {
      subfolders = await getJSON<Item[]>(`/api/frameio/folders?account_id=${accountId}&folder_id=${currentFolder.id}`);
      files = await getJSON<FileItem[]>(`/api/frameio/files?account_id=${accountId}&folder_id=${currentFolder.id}`);
    });
  }
  async function drillInto(folder: Item) {
    folderPath = [...folderPath, folder];
    await loadFolder();
  }
  async function jumpTo(index: number) {
    folderPath = folderPath.slice(0, index + 1);
    await loadFolder();
  }

  async function createFolder() {
    const name = newFolderName.trim();
    if (!name || !currentFolder) return;
    creatingFolder = true;
    onerror?.(null);
    try {
      const res = await fetch("/api/frameio/folders", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ account_id: accountId, parent_folder_id: currentFolder.id, name }),
      });
      if (!res.ok) {
        onerror?.((await res.json().catch(() => ({}))).detail ?? `Could not create folder (${res.status})`);
        return;
      }
      const folder = (await res.json()) as Item;
      newFolderName = "";
      await drillInto(folder);
    } finally {
      creatingFolder = false;
    }
  }

  function toggleFile(f: FileItem) {
    folderShare = null;
    const next = new Map(selected);
    if (next.has(f.id)) next.delete(f.id);
    else next.set(f.id, f.name);
    selected = next;
  }
  function shareFolder() {
    if (!currentFolder) return;
    selected = new Map();
    folderShare = currentFolder;
  }
  function clearSource() {
    selected = new Map();
    folderShare = null;
    recursive = false;
  }
</script>

<div class="space-y-5">
  <ProjectPicker
    bind:accountId
    bind:workspaceId
    selectedProjectId={projectId}
    initialAccountId={initial?.accountId ?? ""}
    initialWorkspaceId={initial?.workspaceId ?? ""}
    onselect={onProjectSelect}
    onscopechange={onScopeReset}
  />

  {#if folderPath.length > 0}
    <div class="rounded-md border border-border bg-surface-2 p-3">
      <div class="flex flex-wrap items-center gap-1 text-sm">
        {#each folderPath as f, i (f.id)}
          {#if i > 0}<span class="text-faint">/</span>{/if}
          <button onclick={() => jumpTo(i)} class="rounded px-1 hover:bg-surface-3 {i === folderPath.length - 1 ? 'font-medium' : 'text-muted'}">{f.name}</button>
        {/each}
        <button onclick={shareFolder} class="ml-auto rounded border border-border bg-surface px-2 py-0.5 text-xs hover:bg-surface-2">Share this whole folder</button>
      </div>
      <div class="mt-3 space-y-1">
        {#if loadingFrameio}<p class="text-xs text-faint">Loading…</p>{/if}
        {#each subfolders as sf (sf.id)}
          <button onclick={() => drillInto(sf)} class="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm hover:bg-surface-3">
            <span class="text-faint">📁</span> {sf.name}
          </button>
        {/each}
        {#each files as f (f.id)}
          <label class="flex w-full cursor-pointer items-center gap-2 rounded px-2 py-1 text-sm hover:bg-surface-3">
            <input type="checkbox" checked={selected.has(f.id)} onchange={() => toggleFile(f)} />
            <span class="text-faint">📄</span> {f.name}
          </label>
        {/each}
        {#if !loadingFrameio && subfolders.length === 0 && files.length === 0}
          <p class="text-xs text-faint">This folder is empty.</p>
        {/if}
      </div>
      <div class="mt-3 flex items-center gap-2">
        <input bind:value={newFolderName} placeholder="New subfolder name"
          onkeydown={(e) => e.key === "Enter" && createFolder()}
          class="flex-1 rounded-md border border-border bg-surface px-2 py-1 text-sm" />
        <button onclick={createFolder} disabled={!newFolderName.trim() || creatingFolder}
          class="shrink-0 rounded-md border border-border px-2.5 py-1 text-xs hover:bg-surface-3 disabled:opacity-50">
          {creatingFolder ? "Creating…" : "＋ New folder"}
        </button>
      </div>
    </div>
  {/if}

  {#if hasSource}
    <div class="rounded-md bg-accent px-3 py-2 text-sm text-on-accent">
      {#if folderShare}
        <div class="flex items-center justify-between">
          <span>Sharing folder: <span class="font-medium">{folderShare.name}</span></span>
          <button onclick={clearSource} class="text-xs text-faint hover:text-on-accent">clear</button>
        </div>
        <label class="mt-1 flex items-center gap-1.5 text-xs text-faint">
          <input type="checkbox" bind:checked={recursive} /> Include subfolders (recursive)
        </label>
      {:else}
        <div class="flex items-center justify-between">
          <span>{selected.size} file{selected.size === 1 ? "" : "s"} selected</span>
          <button onclick={clearSource} class="text-xs text-faint hover:text-on-accent">clear</button>
        </div>
      {/if}
    </div>
  {/if}
</div>
