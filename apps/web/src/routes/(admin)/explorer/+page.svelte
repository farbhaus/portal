<script lang="ts">
  import { goto } from "$app/navigation";
  import { PageHeader, Button, Card } from "$lib/components";

  type Item = { id: string; name: string };
  type FileItem = { id: string; name: string; file_size: number | null };
  type Project = { id: string; name: string; root_folder_id: string | null };

  let accounts = $state<Item[]>([]);
  let workspaces = $state<Item[]>([]);
  let projects = $state<Project[]>([]);
  let subfolders = $state<Item[]>([]);
  let files = $state<FileItem[]>([]);

  let accountId = $state("");
  let workspaceId = $state("");
  let projectId = $state("");
  let folderPath = $state<Item[]>([]);

  let loading = $state(false);
  let error = $state<string | null>(null);
  let busy = $state(false);

  let selectedFiles = $state<Set<string>>(new Set());
  let selectedFolders = $state<Set<string>>(new Set());

  const currentFolder = $derived(folderPath.at(-1) ?? null);
  const selCount = $derived(selectedFiles.size + selectedFolders.size);

  async function getJSON<T>(url: string): Promise<T> {
    const res = await fetch(url);
    if (!res.ok) throw new Error((await res.json().catch(() => ({}))).detail ?? `Failed (${res.status})`);
    return res.json();
  }
  async function load<T>(fn: () => Promise<T>) {
    loading = true;
    error = null;
    try {
      return await fn();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to load from Frame.io";
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load(async () => (accounts = await getJSON<Item[]>("/api/frameio/accounts")));
  });

  function clearSelection() {
    selectedFiles = new Set();
    selectedFolders = new Set();
  }

  async function onAccount() {
    workspaceId = projectId = "";
    workspaces = projects = subfolders = [];
    files = [];
    folderPath = [];
    clearSelection();
    if (!accountId) return;
    await load(async () => (workspaces = await getJSON<Item[]>(`/api/frameio/workspaces?account_id=${accountId}`)));
  }
  async function onWorkspace() {
    projectId = "";
    projects = subfolders = [];
    files = [];
    folderPath = [];
    clearSelection();
    if (!workspaceId) return;
    await load(async () => (projects = await getJSON<Project[]>(`/api/frameio/projects?account_id=${accountId}&workspace_id=${workspaceId}`)));
  }
  async function onProject() {
    subfolders = [];
    files = [];
    folderPath = [];
    clearSelection();
    const proj = projects.find((p) => p.id === projectId);
    if (!proj?.root_folder_id) return;
    folderPath = [{ id: proj.root_folder_id, name: `${proj.name} (root)` }];
    await loadFolder();
  }
  async function loadFolder() {
    if (!currentFolder) return;
    clearSelection();
    await load(async () => {
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

  function toggle(set: Set<string>, id: string): Set<string> {
    const next = new Set(set);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return next;
  }

  function fmtBytes(n: number | null): string {
    if (n == null) return "";
    const u = ["B", "KB", "MB", "GB", "TB"];
    const i = n > 0 ? Math.floor(Math.log(n) / Math.log(1024)) : 0;
    return `${(n / 1024 ** i).toFixed(i === 0 ? 0 : 1)} ${u[i]}`;
  }

  // ── actions ────────────────────────────────────────────────────────────────
  let newFolderName = $state("");
  async function createFolder() {
    const name = newFolderName.trim();
    if (!name || !currentFolder) return;
    busy = true;
    error = null;
    try {
      const res = await fetch("/api/frameio/folders", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ account_id: accountId, parent_folder_id: currentFolder.id, name }),
      });
      if (!res.ok) {
        error = (await res.json().catch(() => ({}))).detail ?? `Could not create folder (${res.status})`;
        return;
      }
      newFolderName = "";
      await loadFolder();
    } finally {
      busy = false;
    }
  }

  async function downloadSelected() {
    busy = true;
    error = null;
    try {
      for (const id of selectedFiles) {
        const { url } = await getJSON<{ url: string }>(
          `/api/frameio/files/${id}/download-url?account_id=${accountId}`,
        );
        const a = document.createElement("a");
        a.href = url;
        a.rel = "noopener";
        document.body.appendChild(a);
        a.click();
        a.remove();
        await new Promise((r) => setTimeout(r, 400)); // stagger so the browser allows each
      }
    } catch (e) {
      error = e instanceof Error ? e.message : "Download failed";
    } finally {
      busy = false;
    }
  }

  async function deleteSelected() {
    if (!confirm(`Permanently delete ${selCount} item${selCount === 1 ? "" : "s"} from Frame.io? This cannot be undone.`))
      return;
    busy = true;
    error = null;
    try {
      for (const id of selectedFiles) {
        await fetch(`/api/frameio/files/${id}?account_id=${accountId}`, { method: "DELETE" });
      }
      for (const id of selectedFolders) {
        await fetch(`/api/frameio/folders/${id}?account_id=${accountId}`, { method: "DELETE" });
      }
      await loadFolder();
    } finally {
      busy = false;
    }
  }

  async function createLink(source: Record<string, unknown>) {
    busy = true;
    error = null;
    try {
      const res = await fetch("/api/download-links", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ source }),
      });
      if (!res.ok) {
        error = (await res.json().catch(() => ({}))).detail ?? `Could not create link (${res.status})`;
        return;
      }
      const link = await res.json();
      await goto(`/download-links/${link.id}`);
    } finally {
      busy = false;
    }
  }

  function linkFromSelection() {
    const ids = [...selectedFiles];
    if (ids.length === 1) return createLink({ type: "file", account_id: accountId, file_id: ids[0] });
    return createLink({ type: "selection", account_id: accountId, file_ids: ids });
  }
  function linkFromCurrentFolder() {
    if (!currentFolder) return;
    return createLink({ type: "folder", account_id: accountId, folder_id: currentFolder.id, recursive: true });
  }
</script>

<div class="space-y-5">
  <PageHeader title="Files" subtitle="Browse your Frame.io library — download, organize, and share without leaving Portal." />

  {#if error}<p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>{/if}

  <Card class="space-y-4">
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-3">
      <label class="block text-sm">
        <span class="text-muted">Account</span>
        <select bind:value={accountId} onchange={onAccount} class="mt-1 w-full rounded-md border border-border px-2 py-1.5">
          <option value="">Select…</option>
          {#each accounts as a (a.id)}<option value={a.id}>{a.name}</option>{/each}
        </select>
      </label>
      <label class="block text-sm">
        <span class="text-muted">Workspace</span>
        <select bind:value={workspaceId} onchange={onWorkspace} disabled={!accountId} class="mt-1 w-full rounded-md border border-border px-2 py-1.5 disabled:bg-surface-2">
          <option value="">Select…</option>
          {#each workspaces as w (w.id)}<option value={w.id}>{w.name}</option>{/each}
        </select>
      </label>
      <label class="block text-sm">
        <span class="text-muted">Project</span>
        <select bind:value={projectId} onchange={onProject} disabled={!workspaceId} class="mt-1 w-full rounded-md border border-border px-2 py-1.5 disabled:bg-surface-2">
          <option value="">Select…</option>
          {#each projects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
        </select>
      </label>
    </div>
  </Card>

  {#if folderPath.length > 0}
    <Card padded={false}>
      <!-- breadcrumb + per-folder actions -->
      <div class="flex flex-wrap items-center gap-1 border-b border-border px-4 py-2.5 text-sm">
        {#each folderPath as f, i (f.id)}
          {#if i > 0}<span class="text-faint">/</span>{/if}
          <button onclick={() => jumpTo(i)} class="rounded px-1 hover:bg-surface-3 {i === folderPath.length - 1 ? 'font-medium' : 'text-muted'}">{f.name}</button>
        {/each}
        <div class="ml-auto flex items-center gap-2">
          <Button variant="ghost" size="sm" onclick={linkFromCurrentFolder} disabled={busy}>Share folder ↗</Button>
        </div>
      </div>

      <!-- selection toolbar -->
      {#if selCount > 0}
        <div class="flex items-center gap-2 border-b border-border bg-surface-2 px-4 py-2 text-sm">
          <span class="text-muted">{selCount} selected</span>
          <div class="ml-auto flex items-center gap-2">
            {#if selectedFiles.size > 0}
              <Button variant="ghost" size="sm" onclick={downloadSelected} disabled={busy}>Download</Button>
              <Button variant="ghost" size="sm" onclick={linkFromSelection} disabled={busy}>Create link</Button>
            {/if}
            <Button variant="danger" size="sm" onclick={deleteSelected} disabled={busy}>Delete</Button>
            <button onclick={clearSelection} class="text-xs text-faint hover:text-text">clear</button>
          </div>
        </div>
      {/if}

      <!-- listing -->
      <div class="divide-y divide-border/60">
        {#if loading}
          <p class="px-4 py-8 text-center text-sm text-faint">Loading…</p>
        {:else}
          {#each subfolders as sf (sf.id)}
            <div class="flex items-center gap-3 px-4 py-2 text-sm hover:bg-surface-2">
              <input type="checkbox" checked={selectedFolders.has(sf.id)} onchange={() => (selectedFolders = toggle(selectedFolders, sf.id))} />
              <button onclick={() => drillInto(sf)} class="flex flex-1 items-center gap-2 text-left">
                <span class="text-faint">📁</span> <span class="truncate font-medium">{sf.name}</span>
              </button>
            </div>
          {/each}
          {#each files as f (f.id)}
            <label class="flex cursor-pointer items-center gap-3 px-4 py-2 text-sm hover:bg-surface-2">
              <input type="checkbox" checked={selectedFiles.has(f.id)} onchange={() => (selectedFiles = toggle(selectedFiles, f.id))} />
              <span class="text-faint">📄</span>
              <span class="flex-1 truncate">{f.name}</span>
              <span class="shrink-0 text-xs text-faint">{fmtBytes(f.file_size)}</span>
            </label>
          {/each}
          {#if subfolders.length === 0 && files.length === 0}
            <p class="px-4 py-8 text-center text-sm text-faint">This folder is empty.</p>
          {/if}
        {/if}
      </div>

      <!-- new folder -->
      <div class="flex items-center gap-2 border-t border-border px-4 py-2.5">
        <input bind:value={newFolderName} placeholder="New subfolder name" onkeydown={(e) => e.key === "Enter" && createFolder()}
          class="flex-1 rounded-md border border-border bg-surface px-2 py-1 text-sm" />
        <Button variant="ghost" size="sm" onclick={createFolder} disabled={busy || !newFolderName.trim()}>＋ New folder</Button>
      </div>
    </Card>
  {:else}
    <p class="text-sm text-faint">Pick an account, workspace, and project to start browsing.</p>
  {/if}
</div>
