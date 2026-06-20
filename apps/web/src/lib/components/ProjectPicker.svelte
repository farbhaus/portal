<script lang="ts">
  // Account + workspace + project picker. Auto-selects the first (main) account and its first
  // (default) workspace so the user never has to re-pick them, then shows a searchable,
  // height-limited project list. Calls `onselect` when a project is chosen, and `onscopechange`
  // whenever the account/workspace changes (so the parent can reset its dependent state).
  type Item = { id: string; name: string };
  type Project = { id: string; name: string; root_folder_id: string | null };

  let {
    accountId = $bindable(""),
    workspaceId = $bindable(""),
    selectedProjectId = "",
    onselect,
    onscopechange,
  }: {
    accountId?: string;
    workspaceId?: string;
    selectedProjectId?: string;
    onselect?: (project: Project) => void;
    onscopechange?: () => void;
  } = $props();

  let accounts = $state<Item[]>([]);
  let workspaces = $state<Item[]>([]);
  let projects = $state<Project[]>([]);
  let search = $state("");
  let loading = $state(false);
  let error = $state<string | null>(null);

  const filtered = $derived(
    projects.filter((p) => p.name.toLowerCase().includes(search.trim().toLowerCase())),
  );

  async function getJSON<T>(url: string): Promise<T> {
    const res = await fetch(url);
    if (!res.ok) throw new Error((await res.json().catch(() => ({}))).detail ?? `Failed (${res.status})`);
    return res.json();
  }
  async function run<T>(fn: () => Promise<T>) {
    loading = true; error = null;
    try { return await fn(); }
    catch (e) { error = e instanceof Error ? e.message : "Failed to load from Frame.io"; }
    finally { loading = false; }
  }

  let started = false;
  $effect(() => {
    if (started) return;
    started = true;
    run(async () => {
      accounts = await getJSON<Item[]>("/api/frameio/accounts");
      if (accounts.length > 0) { accountId = accounts[0].id; await loadWorkspaces(); }
    });
  });

  async function loadWorkspaces() {
    workspaces = await getJSON<Item[]>(`/api/frameio/workspaces?account_id=${accountId}`);
    if (workspaces.length > 0) { workspaceId = workspaces[0].id; await loadProjects(); }
    else { workspaceId = ""; projects = []; }
  }
  async function loadProjects() {
    projects = await getJSON<Project[]>(
      `/api/frameio/projects?account_id=${accountId}&workspace_id=${workspaceId}`,
    );
  }

  async function onAccount() {
    workspaceId = ""; workspaces = []; projects = []; search = "";
    onscopechange?.();
    if (!accountId) return;
    await run(loadWorkspaces);
  }
  async function onWorkspace() {
    projects = []; search = "";
    onscopechange?.();
    if (!workspaceId) return;
    await run(loadProjects);
  }
</script>

<div class="space-y-3">
  {#if error}<p class="rounded bg-danger/10 px-2 py-1 text-xs text-danger">{error}</p>{/if}
  <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
    <label class="block text-sm">
      <span class="text-muted">Account</span>
      <select bind:value={accountId} onchange={onAccount} class="mt-1 w-full rounded-md border border-border px-2 py-1.5">
        {#each accounts as a (a.id)}<option value={a.id}>{a.name}</option>{/each}
      </select>
    </label>
    <label class="block text-sm">
      <span class="text-muted">Workspace</span>
      <select bind:value={workspaceId} onchange={onWorkspace} disabled={!accountId} class="mt-1 w-full rounded-md border border-border px-2 py-1.5 disabled:bg-surface-2">
        {#each workspaces as w (w.id)}<option value={w.id}>{w.name}</option>{/each}
      </select>
    </label>
  </div>
  <div>
    <span class="text-sm text-muted">Project</span>
    <input bind:value={search} placeholder="Search projects…" class="mt-1 mb-2 w-full rounded-md border border-border px-2 py-1.5 text-sm" />
    <div class="max-h-56 divide-y divide-border/60 overflow-y-auto rounded-md border border-border">
      {#if loading && projects.length === 0}
        <p class="px-3 py-2 text-xs text-faint">Loading…</p>
      {:else}
        {#each filtered as p (p.id)}
          <button
            type="button"
            onclick={() => onselect?.(p)}
            class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-surface-2 {selectedProjectId === p.id ? 'bg-surface-2 font-medium' : ''}"
          >
            <span class="text-accent">📁</span>
            <span class="truncate">{p.name}</span>
          </button>
        {/each}
        {#if filtered.length === 0}
          <p class="px-3 py-2 text-xs text-faint">{projects.length === 0 ? "No projects." : "No matches."}</p>
        {/if}
      {/if}
    </div>
  </div>
</div>
