<script lang="ts">
  import { goto } from "$app/navigation";
  import { PageHeader, Button, Card, Modal, CopyButton } from "$lib/components";

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
  // Current project — needed to create Frame.io shares (which are project-scoped).
  let projectId = $state("");
  let projectSearch = $state("");
  // level: 'projects' shows the projects list in the browser; 'folders' shows subfolder/file tree
  let level = $state<"projects" | "folders">("projects");
  let folderPath = $state<Item[]>([]);

  let loading = $state(false);
  let error = $state<string | null>(null);
  let busy = $state(false);

  let selectedFiles = $state<Set<string>>(new Set());
  let selectedFolders = $state<Set<string>>(new Set());

  // Cut/Copy → Paste clipboard. Holds whole items (with names) so paste works after navigating.
  type Clip = { mode: "copy" | "cut"; accountId: string; files: Item[]; folders: Item[] };
  let clipboard = $state<Clip | null>(null);

  const currentFolder = $derived(folderPath.at(-1) ?? null);
  const selCount = $derived(selectedFiles.size + selectedFolders.size);
  const clipCount = $derived(clipboard ? clipboard.files.length + clipboard.folders.length : 0);
  // Show the browser whenever a workspace is picked (so an empty workspace can still create a project).
  const browserVisible = $derived(level === "projects" ? !!workspaceId : folderPath.length > 0);
  const filteredProjects = $derived(
    projects.filter((p) => p.name.toLowerCase().includes(projectSearch.trim().toLowerCase()))
  );

  async function getJSON<T>(url: string): Promise<T> {
    const res = await fetch(url);
    if (!res.ok) throw new Error((await res.json().catch(() => ({}))).detail ?? `Failed (${res.status})`);
    return res.json();
  }
  async function doLoad<T>(fn: () => Promise<T>) {
    loading = true;
    error = null;
    try { return await fn(); }
    catch (e) { error = e instanceof Error ? e.message : "Failed to load from Frame.io"; }
    finally { loading = false; }
  }

  function clearSelection() { selectedFiles = new Set(); selectedFolders = new Set(); }

  // On mount: load accounts and default to the first (main) account + workspace, showing projects.
  $effect(() => {
    doLoad(async () => {
      accounts = await getJSON<Item[]>("/api/frameio/accounts");
      if (accounts.length > 0) {
        accountId = accounts[0].id;
        await loadWorkspaces();
      }
    });
  });

  async function loadWorkspaces() {
    workspaces = await getJSON<Item[]>(`/api/frameio/workspaces?account_id=${accountId}`);
    if (workspaces.length > 0) {
      workspaceId = workspaces[0].id;
      await loadProjects();
    }
  }

  async function loadProjects() {
    projects = await getJSON<Project[]>(
      `/api/frameio/projects?account_id=${accountId}&workspace_id=${workspaceId}`
    );
    level = "projects";
    folderPath = [];
    clearSelection();
  }

  async function onAccount() {
    workspaceId = "";
    workspaces = projects = subfolders = [];
    files = [];
    folderPath = [];
    level = "projects";
    clearSelection();
    if (!accountId) return;
    await doLoad(loadWorkspaces);
  }

  async function onWorkspace() {
    projects = subfolders = [];
    files = [];
    folderPath = [];
    level = "projects";
    clearSelection();
    if (!workspaceId) return;
    await doLoad(loadProjects);
  }

  async function openProject(proj: Project) {
    if (!proj.root_folder_id) return;
    projectId = proj.id;
    folderPath = [{ id: proj.root_folder_id, name: proj.name }];
    level = "folders";
    clearSelection();
    await doLoad(loadFolder);
  }

  async function loadFolder() {
    if (!currentFolder) return;
    clearSelection();
    subfolders = await getJSON<Item[]>(
      `/api/frameio/folders?account_id=${accountId}&folder_id=${currentFolder.id}`
    );
    files = await getJSON<FileItem[]>(
      `/api/frameio/files?account_id=${accountId}&folder_id=${currentFolder.id}`
    );
  }

  async function drillInto(folder: Item) {
    folderPath = [...folderPath, folder];
    await doLoad(loadFolder);
  }

  // Jump in breadcrumb: index 0 = project root → goes back to projects list
  async function jumpTo(index: number) {
    if (index === -1) {
      // back to projects list
      level = "projects";
      folderPath = [];
      clearSelection();
      return;
    }
    folderPath = folderPath.slice(0, index + 1);
    await doLoad(loadFolder);
  }

  function toggle(set: Set<string>, id: string): Set<string> {
    const next = new Set(set);
    if (next.has(id)) next.delete(id); else next.add(id);
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
  let newProjectName = $state("");

  async function createProject() {
    const name = newProjectName.trim();
    if (!name || !workspaceId) return;
    busy = true; error = null;
    try {
      const res = await fetch("/api/frameio/projects", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ account_id: accountId, workspace_id: workspaceId, name }),
      });
      if (!res.ok) { error = (await res.json().catch(() => ({}))).detail ?? `Could not create project (${res.status})`; return; }
      newProjectName = "";
      await doLoad(loadProjects);
    } finally { busy = false; }
  }

  async function deleteProject(proj: Project, e: Event) {
    e.stopPropagation();
    if (!confirm(`Permanently delete project "${proj.name}" and ALL of its contents from Frame.io? This cannot be undone.`)) return;
    busy = true; error = null;
    try {
      const res = await fetch(`/api/frameio/projects/${proj.id}?account_id=${accountId}`, { method: "DELETE" });
      if (!res.ok) { error = (await res.json().catch(() => ({}))).detail ?? `Could not delete project (${res.status})`; return; }
      await doLoad(loadProjects);
    } finally { busy = false; }
  }

  function setClipboard(mode: "copy" | "cut") {
    const fileItems = files.filter((f) => selectedFiles.has(f.id)).map((f) => ({ id: f.id, name: f.name }));
    const folderItems = subfolders.filter((sf) => selectedFolders.has(sf.id));
    clipboard = { mode, accountId, files: fileItems, folders: folderItems };
    clearSelection();
  }
  function clearClipboard() { clipboard = null; }

  async function pasteItem(kind: "files" | "folders", id: string, move: boolean, target: string): Promise<boolean> {
    const res = await fetch(`/api/frameio/${kind}/${id}/${move ? "move" : "copy"}`, {
      method: move ? "PATCH" : "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ account_id: accountId, parent_id: target }),
    });
    if (!res.ok) { error = (await res.json().catch(() => ({}))).detail ?? `Paste failed (${res.status})`; return false; }
    return true;
  }

  async function paste() {
    if (!clipboard || !currentFolder) return;
    if (clipboard.accountId !== accountId) { error = "Can't paste across accounts."; return; }
    const move = clipboard.mode === "cut";
    const target = currentFolder.id;
    busy = true; error = null;
    try {
      for (const f of clipboard.files) if (!(await pasteItem("files", f.id, move, target))) return;
      for (const fo of clipboard.folders) if (!(await pasteItem("folders", fo.id, move, target))) return;
      if (move) clipboard = null;
      await doLoad(loadFolder);
    } finally { busy = false; }
  }

  async function createFolder() {
    const name = newFolderName.trim();
    if (!name || !currentFolder) return;
    busy = true; error = null;
    try {
      const res = await fetch("/api/frameio/folders", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ account_id: accountId, parent_folder_id: currentFolder.id, name }),
      });
      if (!res.ok) { error = (await res.json().catch(() => ({}))).detail ?? `Could not create folder (${res.status})`; return; }
      newFolderName = "";
      await doLoad(loadFolder);
    } finally { busy = false; }
  }

  async function downloadSelected() {
    busy = true; error = null;
    try {
      for (const id of selectedFiles) {
        const { url } = await getJSON<{ url: string }>(
          `/api/frameio/files/${id}/download-url?account_id=${accountId}`
        );
        const a = document.createElement("a");
        a.href = url; a.rel = "noopener";
        document.body.appendChild(a); a.click(); a.remove();
        await new Promise((r) => setTimeout(r, 400));
      }
    } catch (e) {
      error = e instanceof Error ? e.message : "Download failed";
    } finally { busy = false; }
  }

  async function deleteSelected() {
    if (!confirm(`Permanently delete ${selCount} item${selCount === 1 ? "" : "s"} from Frame.io? This cannot be undone.`)) return;
    busy = true; error = null;
    try {
      for (const id of selectedFiles)
        await fetch(`/api/frameio/files/${id}?account_id=${accountId}`, { method: "DELETE" });
      for (const id of selectedFolders)
        await fetch(`/api/frameio/folders/${id}?account_id=${accountId}`, { method: "DELETE" });
      await doLoad(loadFolder);
    } finally { busy = false; }
  }

  async function createLink(source: Record<string, unknown>) {
    busy = true; error = null;
    try {
      const res = await fetch("/api/download-links", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ source }),
      });
      if (!res.ok) { error = (await res.json().catch(() => ({}))).detail ?? `Could not create link (${res.status})`; return; }
      const link = await res.json();
      await goto(`/download-links/${link.id}`);
    } finally { busy = false; }
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

  // ── Frame.io share links (native review page over files/folders) ─────────────
  let shareOpen = $state(false);
  let shareName = $state("");
  let shareAccess = $state<"public" | "secure">("public");
  let sharePassphrase = $state("");
  let shareExpiry = $state("");
  let shareDownloading = $state(true);
  let shareAssetIds = $state<string[]>([]);
  let shareResult = $state<{ short_url: string | null; passphrase: string | null } | null>(null);
  let shareBusy = $state(false);

  function openShare(assetIds: string[], defaultName: string) {
    if (assetIds.length === 0) return;
    shareAssetIds = assetIds;
    shareName = defaultName;
    shareAccess = "public";
    sharePassphrase = "";
    shareExpiry = "";
    shareDownloading = true;
    shareResult = null;
    shareOpen = true;
  }
  function shareFromSelection() {
    openShare([...selectedFolders, ...selectedFiles], currentFolder?.name ?? "Shared files");
  }
  function shareCurrentFolder() {
    if (currentFolder) openShare([currentFolder.id], currentFolder.name);
  }

  async function createShare() {
    if (!projectId) { error = "Open a project first."; return; }
    const name = shareName.trim();
    if (!name) return;
    shareBusy = true; error = null;
    try {
      const body: Record<string, unknown> = {
        account_id: accountId, project_id: projectId, name,
        asset_ids: shareAssetIds, access: shareAccess, downloading_enabled: shareDownloading,
      };
      if (shareExpiry) body.expiration = new Date(shareExpiry).toISOString();
      if (sharePassphrase.trim()) body.passphrase = sharePassphrase.trim();
      const res = await fetch("/api/frameio/shares", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) { error = (await res.json().catch(() => ({}))).detail ?? `Could not create share (${res.status})`; return; }
      const share = await res.json();
      shareResult = { short_url: share.short_url, passphrase: share.passphrase };
      clearSelection();
    } finally { shareBusy = false; }
  }
</script>

<div class="space-y-5">
  <PageHeader title="Files" subtitle="Browse your Frame.io library — download, organize, and share without leaving Portal." />

  {#if error}<p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>{/if}

  <!-- Account + Workspace selectors (2 dropdowns, project is now in the browser) -->
  <Card class="space-y-4">
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
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
    </div>
  </Card>

  {#if browserVisible}
    <Card padded={false}>
      <!-- Breadcrumb -->
      <div class="flex flex-wrap items-center gap-1 border-b border-border px-4 py-2.5 text-sm">
        {#if level === "folders"}
          <button onclick={() => jumpTo(-1)} class="rounded px-1 text-muted hover:bg-surface-3 hover:text-text">Projects</button>
          {#each folderPath as f, i (f.id)}
            <span class="text-faint">/</span>
            <button
              onclick={() => jumpTo(i)}
              class="rounded px-1 hover:bg-surface-3 {i === folderPath.length - 1 ? 'font-medium' : 'text-muted'}"
            >{f.name}</button>
          {/each}
          <div class="ml-auto flex items-center gap-2">
            {#if clipboard}
              <Button variant="ghost" size="sm" onclick={paste} disabled={busy}>Paste {clipCount} ({clipboard.mode})</Button>
              <button onclick={clearClipboard} class="text-xs text-faint hover:text-text" title="Clear clipboard">✕</button>
            {/if}
            <Button variant="ghost" size="sm" onclick={shareCurrentFolder} disabled={busy}>Frame.io share</Button>
            <Button variant="ghost" size="sm" onclick={linkFromCurrentFolder} disabled={busy}>Download link</Button>
          </div>
        {:else}
          <span class="text-sm font-medium text-muted">Projects</span>
          <input bind:value={projectSearch} placeholder="Search projects…" class="ml-auto w-48 rounded-md border border-border bg-surface px-2 py-1 text-xs" />
        {/if}
      </div>

      <!-- Selection toolbar (folders level only) -->
      {#if level === "folders" && selCount > 0}
        <div class="flex items-center gap-2 border-b border-border bg-surface-2 px-4 py-2 text-sm">
          <span class="text-muted">{selCount} selected</span>
          <div class="ml-auto flex items-center gap-2">
            {#if selectedFiles.size > 0}
              <Button variant="ghost" size="sm" onclick={downloadSelected} disabled={busy}>Download</Button>
              <Button variant="ghost" size="sm" onclick={linkFromSelection} disabled={busy}>Download link</Button>
            {/if}
            <Button variant="ghost" size="sm" onclick={shareFromSelection} disabled={busy}>Frame.io share</Button>
            <Button variant="ghost" size="sm" onclick={() => setClipboard("copy")} disabled={busy}>Copy</Button>
            <Button variant="ghost" size="sm" onclick={() => setClipboard("cut")} disabled={busy}>Cut</Button>
            <Button variant="danger" size="sm" onclick={deleteSelected} disabled={busy}>Delete</Button>
            <button onclick={clearSelection} class="text-xs text-faint hover:text-text">clear</button>
          </div>
        </div>
      {/if}

      <!-- Browser listing -->
      <div class="divide-y divide-border/60">
        {#if loading}
          <p class="px-4 py-8 text-center text-sm text-faint">Loading…</p>
        {:else if level === "projects"}
          {#each filteredProjects as proj (proj.id)}
            <div class="flex items-center gap-3 px-4 py-2.5 text-sm hover:bg-surface-2 transition-colors">
              <button onclick={() => openProject(proj)} class="flex flex-1 items-center gap-3 text-left">
                <svg class="h-4 w-4 shrink-0 text-accent" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M4 7a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z" />
                </svg>
                <span class="truncate font-medium">{proj.name}</span>
                <svg class="h-3.5 w-3.5 shrink-0 text-faint" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="m9 18 6-6-6-6" />
                </svg>
              </button>
              <button onclick={(e) => deleteProject(proj, e)} disabled={busy} class="shrink-0 text-xs text-faint hover:text-danger" title="Delete project">Delete</button>
            </div>
          {/each}
          {#if filteredProjects.length === 0 && !loading}
            <p class="px-4 py-8 text-center text-sm text-faint">{projects.length === 0 ? "No projects in this workspace." : "No projects match your search."}</p>
          {/if}
        {:else}
          {#each subfolders as sf (sf.id)}
            <div class="flex items-center gap-3 px-4 py-2 text-sm hover:bg-surface-2 transition-colors">
              <input type="checkbox" checked={selectedFolders.has(sf.id)} onchange={() => (selectedFolders = toggle(selectedFolders, sf.id))} />
              <button onclick={() => drillInto(sf)} class="flex flex-1 items-center gap-2 text-left">
                <span class="text-faint">📁</span>
                <span class="truncate font-medium">{sf.name}</span>
              </button>
            </div>
          {/each}
          {#each files as f (f.id)}
            <label class="flex cursor-pointer items-center gap-3 px-4 py-2 text-sm hover:bg-surface-2 transition-colors">
              <input type="checkbox" checked={selectedFiles.has(f.id)} onchange={() => (selectedFiles = toggle(selectedFiles, f.id))} />
              <span class="text-faint">📄</span>
              <span class="flex-1 truncate">{f.name}</span>
              <span class="shrink-0 text-xs text-faint">{fmtBytes(f.file_size)}</span>
            </label>
          {/each}
          {#if subfolders.length === 0 && files.length === 0 && !loading}
            <p class="px-4 py-8 text-center text-sm text-faint">This folder is empty.</p>
          {/if}
        {/if}
      </div>

      <!-- New folder / new project footer -->
      {#if level === "folders"}
        <div class="flex items-center gap-2 border-t border-border px-4 py-2.5">
          <input bind:value={newFolderName} placeholder="New subfolder name"
            onkeydown={(e) => e.key === "Enter" && createFolder()}
            class="flex-1 rounded-md border border-border bg-surface px-2 py-1 text-sm" />
          <Button variant="ghost" size="sm" onclick={createFolder} disabled={busy || !newFolderName.trim()}>＋ New folder</Button>
        </div>
      {:else if level === "projects" && workspaceId}
        <div class="flex items-center gap-2 border-t border-border px-4 py-2.5">
          <input bind:value={newProjectName} placeholder="New project name"
            onkeydown={(e) => e.key === "Enter" && createProject()}
            class="flex-1 rounded-md border border-border bg-surface px-2 py-1 text-sm" />
          <Button variant="ghost" size="sm" onclick={createProject} disabled={busy || !newProjectName.trim()}>＋ New project</Button>
        </div>
      {/if}
    </Card>
  {:else if workspaceId}
    <p class="text-sm text-faint">Loading projects…</p>
  {:else if accountId}
    <p class="text-sm text-faint">Select a workspace to browse files.</p>
  {:else}
    <p class="text-sm text-faint">Connecting to Frame.io…</p>
  {/if}
</div>

<Modal bind:open={shareOpen} title="Create Frame.io share link">
  {#if shareResult}
    <div class="space-y-3 text-sm">
      <p class="text-muted">Share created — this is a Frame.io review page, separate from a Portal download link.</p>
      {#if shareResult.short_url}
        <div class="rounded-md border border-border bg-surface-2 px-3 py-2">
          <CopyButton value={shareResult.short_url} />
        </div>
      {:else}
        <p class="text-faint">No public URL was returned.</p>
      {/if}
      {#if shareResult.passphrase}
        <p class="text-xs text-muted">Passphrase: <span class="font-mono text-text">{shareResult.passphrase}</span></p>
      {/if}
    </div>
  {:else}
    <div class="space-y-3 text-sm">
      <label class="block">
        <span class="text-muted">Name</span>
        <input bind:value={shareName} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
      </label>
      <label class="block">
        <span class="text-muted">Access</span>
        <select bind:value={shareAccess} class="mt-1 w-full rounded-md border border-border px-2 py-1.5">
          <option value="public">Public — anyone with the link</option>
          <option value="secure">Secure — recipient must sign in</option>
        </select>
      </label>
      <label class="block">
        <span class="text-muted">Passphrase (optional)</span>
        <input bind:value={sharePassphrase} placeholder="none" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
      </label>
      <label class="block">
        <span class="text-muted">Expires (optional)</span>
        <input type="datetime-local" bind:value={shareExpiry} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
      </label>
      <label class="flex items-center gap-2">
        <input type="checkbox" bind:checked={shareDownloading} /> <span class="text-muted">Allow downloading</span>
      </label>
      <p class="text-xs text-faint">Sharing {shareAssetIds.length} item{shareAssetIds.length === 1 ? "" : "s"}.</p>
    </div>
  {/if}
  {#snippet footer()}
    {#if shareResult}
      <Button variant="ghost" size="sm" onclick={() => (shareOpen = false)}>Done</Button>
    {:else}
      <Button variant="ghost" size="sm" onclick={() => (shareOpen = false)} disabled={shareBusy}>Cancel</Button>
      <Button size="sm" onclick={createShare} disabled={shareBusy || !shareName.trim()}>{shareBusy ? "Creating…" : "Create share"}</Button>
    {/if}
  {/snippet}
</Modal>
