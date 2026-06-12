<script lang="ts">
  import { goto } from "$app/navigation";

  type Item = { id: string; name: string };
  type Project = { id: string; name: string; root_folder_id: string | null };

  let accounts = $state<Item[]>([]);
  let workspaces = $state<Item[]>([]);
  let projects = $state<Project[]>([]);
  let subfolders = $state<Item[]>([]);

  let accountId = $state("");
  let workspaceId = $state("");
  let projectId = $state("");
  let folderPath = $state<Item[]>([]);
  let chosenFolder = $state<Item | null>(null);

  // Options
  let name = $state("");
  let destinationPath = $state("");
  let recursive = $state(true);
  let conflictPolicy = $state("rename_suffix");
  let pathTemplate = $state("");
  let enabled = $state(true);

  let loading = $state(false);
  let error = $state<string | null>(null);
  let saving = $state(false);

  const currentFolder = $derived(folderPath.at(-1) ?? null);

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
    workspaces = projects = subfolders = [];
    folderPath = [];
    chosenFolder = null;
    if (!accountId) return;
    await load(async () => {
      workspaces = await getJSON<Item[]>(`/api/frameio/workspaces?account_id=${accountId}`);
    });
  }
  async function onWorkspace() {
    projectId = "";
    projects = subfolders = [];
    folderPath = [];
    chosenFolder = null;
    if (!workspaceId) return;
    await load(async () => {
      projects = await getJSON<Project[]>(
        `/api/frameio/projects?account_id=${accountId}&workspace_id=${workspaceId}`,
      );
    });
  }
  async function onProject() {
    subfolders = [];
    folderPath = [];
    chosenFolder = null;
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

  function chooseFolder() {
    chosenFolder = currentFolder;
    if (!name.trim() && chosenFolder) name = chosenFolder.name;
  }

  async function create() {
    if (!chosenFolder) {
      error = "Pick the folder to watch.";
      return;
    }
    if (!destinationPath.trim()) {
      error = "Set a local destination path.";
      return;
    }
    saving = true;
    error = null;
    try {
      const projectName = projects.find((p) => p.id === projectId)?.name ?? "";
      const body = {
        name: name.trim() || chosenFolder.name,
        source: {
          type: "frameio",
          account_id: accountId,
          workspace_id: workspaceId,
          project_id: projectId,
          folder_id: chosenFolder.id,
          recursive,
          // Sent so the backend skips the tightly rate-limited folder lookup on create.
          folder_name: chosenFolder.name,
          project_name: projectName,
        },
        destination_path: destinationPath.trim(),
        conflict_policy: conflictPolicy,
        path_template: pathTemplate.trim() || null,
        enabled,
      };
      const res = await fetch("/api/sync-rules", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) {
        error = (await res.json().catch(() => ({}))).detail ?? `Could not create (${res.status})`;
        return;
      }
      await goto("/sync-rules");
    } finally {
      saving = false;
    }
  }
</script>

<div class="max-w-2xl space-y-6">
  <div>
    <a href="/sync-rules" class="text-sm text-muted hover:text-text">← Sync rules</a>
    <h1 class="mt-1 text-2xl font-semibold">New sync rule</h1>
  </div>

  {#if error}<p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>{/if}

  <div class="space-y-5 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Source folder</h2>
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
          <button onclick={chooseFolder} class="ml-auto rounded border border-border bg-surface px-2 py-0.5 text-xs hover:bg-surface-2">Watch this folder</button>
        </div>
        <div class="mt-3 space-y-1">
          {#if loading}<p class="text-xs text-faint">Loading…</p>{/if}
          {#each subfolders as sf (sf.id)}
            <button onclick={() => drillInto(sf)} class="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm hover:bg-surface-3">
              <span class="text-faint">📁</span> {sf.name}
            </button>
          {/each}
          {#if !loading && subfolders.length === 0}<p class="text-xs text-faint">No subfolders.</p>{/if}
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

    {#if chosenFolder}
      <div class="flex items-center justify-between rounded-md bg-accent px-3 py-2 text-sm text-on-accent">
        <span>Watching: <span class="font-medium">{chosenFolder.name}</span></span>
        <label class="flex items-center gap-1.5 text-xs text-faint">
          <input type="checkbox" bind:checked={recursive} /> Include subfolders
        </label>
      </div>
    {/if}
  </div>

  <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Options</h2>
    <label class="block text-sm">
      <span class="text-muted">Rule name</span>
      <input bind:value={name} placeholder="defaults to the folder name" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-muted">Destination path (on the Portal host)</span>
      <input bind:value={destinationPath} placeholder="/data/incoming" class="mt-1 w-full rounded-md border border-border px-2 py-1.5 font-mono" />
    </label>
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <label class="block text-sm">
        <span class="text-muted">On filename conflict</span>
        <select bind:value={conflictPolicy} class="mt-1 w-full rounded-md border border-border px-2 py-1.5">
          <option value="rename_suffix">Rename (add suffix)</option>
          <option value="skip">Skip</option>
          <option value="overwrite">Overwrite</option>
        </select>
      </label>
      <label class="block text-sm">
        <span class="text-muted">Path template (optional)</span>
        <input bind:value={pathTemplate} placeholder="{'{project}/{date}/{filename}'}" class="mt-1 w-full rounded-md border border-border px-2 py-1.5 font-mono" />
      </label>
    </div>
    <p class="text-xs text-faint">Tokens: {"{filename} {stem} {ext} {project} {folder} {date} {year} {month} {day}"}. Leave blank to keep original names at the destination root.</p>
    <div class="rounded-md border border-border bg-surface-2 p-3 text-xs text-muted">
      <div class="mb-1 font-medium text-faint">Examples</div>
      <ul class="space-y-0.5 font-mono">
        <li>{"{project}/{date}/{filename}"} → Acme/2026-06-12/clip.mov</li>
        <li>{"{year}/{month}/{filename}"} → 2026/06/clip.mov</li>
        <li>{"{folder}/{stem}{ext}"} → Dailies/clip.mov</li>
      </ul>
    </div>
    <label class="flex items-center gap-1.5 text-sm">
      <input type="checkbox" bind:checked={enabled} /> Enable now (creates the Frame.io webhook)
    </label>
  </div>

  <div class="flex items-center gap-3">
    <button onclick={create} disabled={saving || !chosenFolder} class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50">
      {saving ? "Creating…" : "Create rule"}
    </button>
    <a href="/sync-rules" class="text-sm text-muted hover:text-text">Cancel</a>
  </div>
</div>
