<script lang="ts">
  import { goto } from "$app/navigation";

  type Item = { id: string; name: string };
  type Project = { id: string; name: string; root_folder_id: string | null };
  type FileItem = { id: string; name: string; file_size: number | null };

  let accounts = $state<Item[]>([]);
  let workspaces = $state<Item[]>([]);
  let projects = $state<Project[]>([]);
  let subfolders = $state<Item[]>([]);
  let files = $state<FileItem[]>([]);

  let accountId = $state("");
  let workspaceId = $state("");
  let projectId = $state("");
  let folderPath = $state<Item[]>([]);

  // Source selection: a basket of individual files, OR a whole folder.
  let selected = $state<Map<string, string>>(new Map()); // file_id -> name
  let folderShare = $state<Item | null>(null);
  let recursive = $state(false);

  // Options
  let expiresAt = $state("");
  let password = $state("");
  let maxDownloads = $state("");
  let reqName = $state(false);
  let reqEmail = $state(false);
  let verifyEmail = $state(false);
  let allowPreview = $state(true);
  let brandDisplayName = $state("");
  let brandSubtitle = $state("");

  let loading = $state(false);
  let error = $state<string | null>(null);
  let saving = $state(false);

  const currentFolder = $derived(folderPath.at(-1) ?? null);
  const hasSource = $derived(folderShare !== null || selected.size > 0);

  async function getJSON<T>(url: string): Promise<T> {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`Request failed (${res.status})`);
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
    load(async () => {
      accounts = await getJSON<Item[]>("/api/frameio/accounts");
    });
  });

  async function onAccount() {
    workspaceId = projectId = "";
    workspaces = projects = subfolders = files = [];
    folderPath = [];
    if (!accountId) return;
    await load(async () => {
      workspaces = await getJSON<Item[]>(`/api/frameio/workspaces?account_id=${accountId}`);
    });
  }
  async function onWorkspace() {
    projectId = "";
    projects = subfolders = files = [];
    folderPath = [];
    if (!workspaceId) return;
    await load(async () => {
      projects = await getJSON<Project[]>(
        `/api/frameio/projects?account_id=${accountId}&workspace_id=${workspaceId}`,
      );
    });
  }
  async function onProject() {
    subfolders = files = [];
    folderPath = [];
    const proj = projects.find((p) => p.id === projectId);
    if (!proj?.root_folder_id) return;
    folderPath = [{ id: proj.root_folder_id, name: `${proj.name} (root)` }];
    await loadFolder();
  }
  async function loadFolder() {
    if (!currentFolder) return;
    await load(async () => {
      subfolders = await getJSON<Item[]>(
        `/api/frameio/folders?account_id=${accountId}&folder_id=${currentFolder.id}`,
      );
      files = await getJSON<FileItem[]>(
        `/api/frameio/files?account_id=${accountId}&folder_id=${currentFolder.id}`,
      );
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

  let newFolderName = $state("");
  let creatingFolder = $state(false);
  async function createFolder() {
    const name = newFolderName.trim();
    if (!name || !currentFolder) return;
    creatingFolder = true;
    error = null;
    try {
      const res = await fetch("/api/frameio/folders", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ account_id: accountId, parent_folder_id: currentFolder.id, name }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        error = body.detail ?? `Could not create folder (${res.status})`;
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

  function buildSource(): Record<string, unknown> | null {
    if (folderShare) {
      return { type: "folder", account_id: accountId, folder_id: folderShare.id, recursive };
    }
    const ids = [...selected.keys()];
    if (ids.length === 1) return { type: "file", account_id: accountId, file_id: ids[0] };
    if (ids.length > 1) return { type: "selection", account_id: accountId, file_ids: ids };
    return null;
  }

  async function create() {
    const source = buildSource();
    if (!source) {
      error = "Pick a file, some files, or a folder to share.";
      return;
    }
    saving = true;
    error = null;
    try {
      const body: Record<string, unknown> = {
        source,
        viewer_fields_required: { name: reqName, email: reqEmail },
        verify_email: verifyEmail,
        allow_preview: allowPreview,
      };
      if (expiresAt) body.expires_at = new Date(expiresAt).toISOString();
      if (password) body.password = password;
      if (maxDownloads) body.max_downloads = parseInt(maxDownloads, 10);
      if (brandDisplayName.trim()) body.brand_display_name = brandDisplayName.trim();
      if (brandSubtitle.trim()) body.brand_subtitle = brandSubtitle.trim();

      const res = await fetch("/api/download-links", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) {
        error = (await res.json().catch(() => ({}))).detail ?? `Could not create (${res.status})`;
        return;
      }
      await goto("/download-links");
    } finally {
      saving = false;
    }
  }
</script>

<div class="mx-auto max-w-2xl space-y-6">
  <div>
    <a href="/download-links" class="text-sm text-muted hover:text-text">← Download links</a>
    <h1 class="mt-1 text-2xl font-semibold">New download link</h1>
  </div>

  {#if error}<p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>{/if}

  <div class="space-y-5 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Source</h2>
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
          {#if loading}<p class="text-xs text-faint">Loading…</p>{/if}
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
          {#if !loading && subfolders.length === 0 && files.length === 0}
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

    <!-- Chosen source summary -->
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

  <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Options</h2>
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-3">
      <label class="block text-sm">
        <span class="text-muted">Expires</span>
        <input type="datetime-local" bind:value={expiresAt} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
      </label>
      <label class="block text-sm">
        <span class="text-muted">Password</span>
        <input type="text" bind:value={password} placeholder="none" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
      </label>
      <label class="block text-sm">
        <span class="text-muted">Max downloads</span>
        <input type="number" min="1" bind:value={maxDownloads} placeholder="unlimited" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
      </label>
    </div>
    <div class="flex flex-wrap gap-4 text-sm">
      <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={reqName} /> Require name</label>
      <label class="flex items-center gap-1.5"><input type="checkbox" checked={reqEmail || verifyEmail} disabled={verifyEmail} onchange={(e) => (reqEmail = e.currentTarget.checked)} /> Require email</label>
      <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={verifyEmail} /> Require OTP (one-time passcode by email)</label>
      <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={allowPreview} /> Show thumbnails/previews</label>
    </div>
  </div>

  <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Branding</h2>
    <label class="block text-sm">
      <span class="text-muted">Display name</span>
      <input bind:value={brandDisplayName} placeholder="e.g. Your deliverables" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-muted">Subtitle</span>
      <input bind:value={brandSubtitle} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
    </label>
  </div>

  <div class="flex items-center gap-3">
    <button onclick={create} disabled={saving || !hasSource} class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50">
      {saving ? "Creating…" : "Create link"}
    </button>
    <a href="/download-links" class="text-sm text-muted hover:text-text">Cancel</a>
  </div>
</div>
