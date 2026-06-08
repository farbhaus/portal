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
  // Breadcrumb from the project root down to the currently-browsed folder (the target).
  let folderPath = $state<Item[]>([]);

  let displayName = $state("");
  let subtitle = $state("");
  let logoUrl = $state("");
  let accentColor = $state("#111111");

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
      return undefined;
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
    if (!accountId) return;
    await load(async () => {
      workspaces = await getJSON<Item[]>(`/api/frameio/workspaces?account_id=${accountId}`);
    });
  }

  async function onWorkspace() {
    projectId = "";
    projects = subfolders = [];
    folderPath = [];
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
    const proj = projects.find((p) => p.id === projectId);
    if (!proj?.root_folder_id) return;
    folderPath = [{ id: proj.root_folder_id, name: `${proj.name} (root)` }];
    await loadSubfolders();
  }

  async function loadSubfolders() {
    if (!currentFolder) return;
    await load(async () => {
      subfolders = await getJSON<Item[]>(
        `/api/frameio/folders?account_id=${accountId}&folder_id=${currentFolder.id}`,
      );
    });
  }

  async function drillInto(folder: Item) {
    folderPath = [...folderPath, folder];
    await loadSubfolders();
  }

  async function jumpTo(index: number) {
    folderPath = folderPath.slice(0, index + 1);
    await loadSubfolders();
  }

  async function save() {
    if (!displayName.trim() || !currentFolder) return;
    saving = true;
    error = null;
    try {
      const res = await fetch("/api/destinations", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          display_name: displayName.trim(),
          config: {
            type: "frameio",
            account_id: accountId,
            workspace_id: workspaceId,
            project_id: projectId,
            folder_id: currentFolder.id,
          },
          subtitle: subtitle.trim() || null,
          logo_url: logoUrl.trim() || null,
          accent_color: accentColor || null,
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        error = body.detail ?? `Could not create destination (${res.status})`;
        return;
      }
      await goto("/destinations");
    } finally {
      saving = false;
    }
  }
</script>

<div class="max-w-2xl space-y-6">
  <div>
    <a href="/destinations" class="text-sm text-neutral-500 hover:text-neutral-900">← Destinations</a>
    <h1 class="mt-1 text-2xl font-semibold">New destination</h1>
  </div>

  {#if error}
    <p class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">{error}</p>
  {/if}

  <div class="space-y-5 rounded-xl border border-neutral-200 bg-white p-6">
    <h2 class="font-medium">Frame.io folder</h2>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-3">
      <label class="block text-sm">
        <span class="text-neutral-500">Account</span>
        <select bind:value={accountId} onchange={onAccount} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5">
          <option value="">Select…</option>
          {#each accounts as a (a.id)}<option value={a.id}>{a.name}</option>{/each}
        </select>
      </label>
      <label class="block text-sm">
        <span class="text-neutral-500">Workspace</span>
        <select bind:value={workspaceId} onchange={onWorkspace} disabled={!accountId} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5 disabled:bg-neutral-50">
          <option value="">Select…</option>
          {#each workspaces as w (w.id)}<option value={w.id}>{w.name}</option>{/each}
        </select>
      </label>
      <label class="block text-sm">
        <span class="text-neutral-500">Project</span>
        <select bind:value={projectId} onchange={onProject} disabled={!workspaceId} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5 disabled:bg-neutral-50">
          <option value="">Select…</option>
          {#each projects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
        </select>
      </label>
    </div>

    {#if folderPath.length > 0}
      <div class="rounded-md border border-neutral-200 bg-neutral-50 p-3">
        <div class="flex flex-wrap items-center gap-1 text-sm">
          {#each folderPath as f, i (f.id)}
            {#if i > 0}<span class="text-neutral-400">/</span>{/if}
            <button onclick={() => jumpTo(i)} class="rounded px-1 hover:bg-neutral-200 {i === folderPath.length - 1 ? 'font-medium' : 'text-neutral-500'}">
              {f.name}
            </button>
          {/each}
        </div>
        <div class="mt-3 space-y-1">
          {#if subfolders.length === 0}
            <p class="text-xs text-neutral-400">{loading ? "Loading…" : "No subfolders. This folder will be the target."}</p>
          {:else}
            {#each subfolders as sf (sf.id)}
              <button onclick={() => drillInto(sf)} class="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm hover:bg-neutral-200">
                <span class="text-neutral-400">📁</span> {sf.name}
              </button>
            {/each}
          {/if}
        </div>
        <p class="mt-3 text-xs text-neutral-600">
          Target folder: <span class="font-medium">{currentFolder?.name}</span>
        </p>
      </div>
    {/if}
  </div>

  <div class="space-y-4 rounded-xl border border-neutral-200 bg-white p-6">
    <h2 class="font-medium">Branding</h2>
    <label class="block text-sm">
      <span class="text-neutral-500">Display name <span class="text-red-500">*</span></span>
      <input bind:value={displayName} placeholder="e.g. DIT Drop — Project X" class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-neutral-500">Subtitle</span>
      <input bind:value={subtitle} placeholder="Shown on the upload page" class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-neutral-500">Logo URL</span>
      <input bind:value={logoUrl} placeholder="https://…" class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-neutral-500">Accent color</span>
      <input type="color" bind:value={accentColor} class="mt-1 block h-9 w-16 rounded-md border border-neutral-300" />
    </label>
  </div>

  <div class="flex items-center gap-3">
    <button
      onclick={save}
      disabled={saving || !displayName.trim() || !currentFolder}
      class="rounded-md bg-neutral-900 px-4 py-2 text-sm font-medium text-white hover:bg-neutral-800 disabled:opacity-50"
    >
      {saving ? "Creating…" : "Create destination"}
    </button>
    <a href="/destinations" class="text-sm text-neutral-500 hover:text-neutral-900">Cancel</a>
  </div>
</div>
