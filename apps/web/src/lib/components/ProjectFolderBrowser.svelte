<script lang="ts" module>
  export type FolderItem = { id: string; name: string };
  export type FolderBrowserInit = {
    accountId: string;
    workspaceId: string;
    projectId: string;
    projectName: string;
    folderPath: FolderItem[];
  };
</script>

<script lang="ts">
  // Account/workspace/project scope + folder drill-down + inline "new folder" — extracted from
  // the destination and sync-rule create pages so the upload-link wizard can embed it too. The
  // parent owns the selection via bindables; the browsed folder is `folderPath.at(-1)`.
  // `initial` seeds the browser at a known location (e.g. "+ Sync rule here" on a destination)
  // and must be set before mount; `crumbAction` renders at the end of the breadcrumb row.
  import type { Snippet } from "svelte";
  import PathBreadcrumb from "./PathBreadcrumb.svelte";
  import ProjectPicker from "./ProjectPicker.svelte";

  type Project = { id: string; name: string; root_folder_id: string | null };

  let {
    accountId = $bindable(""),
    workspaceId = $bindable(""),
    projectId = $bindable(""),
    projectName = $bindable(""),
    folderPath = $bindable([]),
    initial,
    crumbAction,
  }: {
    accountId?: string;
    workspaceId?: string;
    projectId?: string;
    projectName?: string;
    folderPath?: FolderItem[];
    initial?: FolderBrowserInit;
    crumbAction?: Snippet;
  } = $props();

  let subfolders = $state<FolderItem[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

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

  let seeded = false;
  $effect(() => {
    if (seeded || !initial) return;
    seeded = true;
    accountId = initial.accountId;
    workspaceId = initial.workspaceId;
    projectId = initial.projectId;
    projectName = initial.projectName;
    folderPath = [...initial.folderPath];
    void loadSubfolders();
  });

  function onScopeReset() {
    projectId = "";
    projectName = "";
    subfolders = [];
    folderPath = [];
  }
  async function onProjectSelect(proj: Project) {
    projectId = proj.id;
    projectName = proj.name;
    subfolders = [];
    folderPath = [];
    if (!proj.root_folder_id) return;
    folderPath = [{ id: proj.root_folder_id, name: `${proj.name} (root)` }];
    await loadSubfolders();
  }

  async function loadSubfolders() {
    if (!currentFolder) return;
    await load(async () => {
      subfolders = await getJSON<FolderItem[]>(
        `/api/frameio/folders?account_id=${accountId}&folder_id=${currentFolder.id}`,
      );
    });
  }
  async function drillInto(folder: FolderItem) {
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
      const folder = (await res.json()) as FolderItem;
      newFolderName = "";
      await drillInto(folder); // make the new folder the browsed target
    } finally {
      creatingFolder = false;
    }
  }
</script>

<div class="space-y-5">
  {#if error}
    <p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
  {/if}

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
        <PathBreadcrumb segments={folderPath} onnavigate={jumpTo} class="text-sm" />
        {#if crumbAction}{@render crumbAction()}{/if}
      </div>
      <div class="mt-3 space-y-1">
        {#if loading}
          <p class="text-xs text-faint">Loading…</p>
        {:else if subfolders.length === 0}
          <p class="text-xs text-faint">No subfolders.</p>
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
    </div>
  {/if}
</div>
