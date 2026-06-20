<script lang="ts">
  import { goto } from "$app/navigation";
  import { ProjectPicker } from "$lib/components";

  type Item = { id: string; name: string };
  type Project = { id: string; name: string; root_folder_id: string | null };

  let subfolders = $state<Item[]>([]);

  let accountId = $state("");
  let workspaceId = $state("");
  let projectId = $state("");
  // Breadcrumb from the project root down to the currently-browsed folder (the target).
  let folderPath = $state<Item[]>([]);

  let displayName = $state("");
  let subtitle = $state("");

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

  function onScopeReset() {
    projectId = "";
    subfolders = [];
    folderPath = [];
  }
  async function onProjectSelect(proj: Project) {
    projectId = proj.id;
    subfolders = [];
    folderPath = [];
    if (!proj.root_folder_id) return;
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
      await drillInto(folder); // make the new folder the selected target
    } finally {
      creatingFolder = false;
    }
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

<div class="mx-auto max-w-2xl space-y-6">
  <div>
    <a href="/destinations" class="text-sm text-muted hover:text-text">← Destinations</a>
    <h1 class="mt-1 text-2xl font-semibold">New destination</h1>
  </div>

  {#if error}
    <p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
  {/if}

  <div class="space-y-5 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Frame.io folder</h2>

    <ProjectPicker
      bind:accountId
      bind:workspaceId
      selectedProjectId={projectId}
      onselect={onProjectSelect}
      onscopechange={onScopeReset}
    />

    {#if folderPath.length > 0}
      <div class="rounded-md border border-border bg-surface-2 p-3">
        <div class="flex flex-wrap items-center gap-1 text-sm">
          {#each folderPath as f, i (f.id)}
            {#if i > 0}<span class="text-faint">/</span>{/if}
            <button onclick={() => jumpTo(i)} class="rounded px-1 hover:bg-surface-3 {i === folderPath.length - 1 ? 'font-medium' : 'text-muted'}">
              {f.name}
            </button>
          {/each}
        </div>
        <div class="mt-3 space-y-1">
          {#if subfolders.length === 0}
            <p class="text-xs text-faint">{loading ? "Loading…" : "No subfolders. This folder will be the target."}</p>
          {:else}
            {#each subfolders as sf (sf.id)}
              <button onclick={() => drillInto(sf)} class="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm hover:bg-surface-3">
                <span class="text-faint">📁</span> {sf.name}
              </button>
            {/each}
          {/if}
        </div>
        <div class="mt-3 flex items-center gap-2">
          <input
            bind:value={newFolderName}
            placeholder="New subfolder name"
            onkeydown={(e) => e.key === "Enter" && createFolder()}
            class="flex-1 rounded-md border border-border bg-surface px-2 py-1 text-sm"
          />
          <button onclick={createFolder} disabled={!newFolderName.trim() || creatingFolder}
            class="shrink-0 rounded-md border border-border px-2.5 py-1 text-xs hover:bg-surface-3 disabled:opacity-50">
            {creatingFolder ? "Creating…" : "＋ New folder"}
          </button>
        </div>
        <p class="mt-3 text-xs text-muted">
          Target folder: <span class="font-medium">{currentFolder?.name}</span>
        </p>
      </div>
    {/if}
  </div>

  <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Branding</h2>
    <label class="block text-sm">
      <span class="text-muted">Display name <span class="text-danger">*</span></span>
      <input bind:value={displayName} placeholder="e.g. DIT Drop — Project X" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-muted">Subtitle</span>
      <input bind:value={subtitle} placeholder="Shown on the upload page" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
    </label>
  </div>

  <div class="flex items-center gap-3">
    <button
      onclick={save}
      disabled={saving || !displayName.trim() || !currentFolder}
      class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50"
    >
      {saving ? "Creating…" : "Create destination"}
    </button>
    <a href="/destinations" class="text-sm text-muted hover:text-text">Cancel</a>
  </div>
</div>
