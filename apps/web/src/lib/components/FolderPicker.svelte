<script lang="ts">
  // Drill-down Frame.io folder picker, scoped to one account subtree (e.g. a destination).
  // The currently-navigated folder is the selection; bind `value` to read it.
  import { untrack } from "svelte";

  type Item = { id: string; name: string };

  let {
    accountId,
    rootFolderId,
    rootName = "Destination root",
    value = $bindable<Item | null>(null),
  }: {
    accountId: string;
    rootFolderId: string;
    rootName?: string;
    value?: Item | null;
  } = $props();

  let path = $state<Item[]>([]);
  let subfolders = $state<Item[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let newName = $state("");
  let creating = $state(false);

  const current = $derived(path.at(-1) ?? null);

  async function getJSON<T>(url: string): Promise<T> {
    const res = await fetch(url);
    if (!res.ok) throw new Error((await res.json().catch(() => ({}))).detail ?? `Failed (${res.status})`);
    return res.json();
  }

  // Re-initialize whenever the account/root changes (e.g. a different destination is selected).
  $effect(() => {
    const a = accountId;
    const r = rootFolderId;
    untrack(() => {
      if (!a || !r) { path = []; subfolders = []; value = null; return; }
      path = [{ id: r, name: rootName }];
      value = { id: r, name: rootName };
      void load();
    });
  });

  async function load() {
    if (!current) return;
    loading = true; error = null;
    try {
      subfolders = await getJSON<Item[]>(`/api/frameio/folders?account_id=${accountId}&folder_id=${current.id}`);
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to load folders";
    } finally {
      loading = false;
    }
  }

  async function drillInto(f: Item) {
    path = [...path, f];
    value = { id: f.id, name: f.name };
    await load();
  }
  async function jumpTo(i: number) {
    path = path.slice(0, i + 1);
    value = current ? { id: current.id, name: current.name } : null;
    await load();
  }

  async function createFolder() {
    const name = newName.trim();
    if (!name || !current) return;
    creating = true; error = null;
    try {
      const res = await fetch("/api/frameio/folders", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ account_id: accountId, parent_folder_id: current.id, name }),
      });
      if (!res.ok) { error = (await res.json().catch(() => ({}))).detail ?? `Could not create folder (${res.status})`; return; }
      const folder = (await res.json()) as Item;
      newName = "";
      await drillInto(folder);
    } finally {
      creating = false;
    }
  }
</script>

<div class="rounded-md border border-border bg-surface-2 p-3 text-sm">
  {#if error}<p class="mb-2 rounded bg-danger/10 px-2 py-1 text-xs text-danger">{error}</p>{/if}

  <div class="flex flex-wrap items-center gap-1">
    {#each path as f, i (f.id)}
      {#if i > 0}<span class="text-faint">/</span>{/if}
      <button type="button" onclick={() => jumpTo(i)} class="rounded px-1 hover:bg-surface-3 {i === path.length - 1 ? 'font-medium' : 'text-muted'}">{f.name}</button>
    {/each}
  </div>

  <div class="mt-2 max-h-48 space-y-1 overflow-y-auto">
    {#if loading}
      <p class="text-xs text-faint">Loading…</p>
    {:else}
      {#each subfolders as sf (sf.id)}
        <button type="button" onclick={() => drillInto(sf)} class="flex w-full items-center gap-2 rounded px-2 py-1 text-left hover:bg-surface-3">
          <span class="text-faint">📁</span> {sf.name}
        </button>
      {/each}
      {#if subfolders.length === 0}
        <p class="text-xs text-faint">No subfolders here.</p>
      {/if}
    {/if}
  </div>

  <div class="mt-2 flex items-center gap-2">
    <input bind:value={newName} placeholder="New subfolder name"
      onkeydown={(e) => e.key === "Enter" && (e.preventDefault(), createFolder())}
      class="flex-1 rounded-md border border-border bg-surface px-2 py-1 text-xs" />
    <button type="button" onclick={createFolder} disabled={!newName.trim() || creating}
      class="shrink-0 rounded-md border border-border px-2.5 py-1 text-xs hover:bg-surface-3 disabled:opacity-50">
      {creating ? "Creating…" : "＋ New folder"}
    </button>
  </div>

  {#if value}
    <p class="mt-2 text-xs text-muted">Uploads land in: <span class="font-medium text-text">{value.name}</span></p>
  {/if}
</div>
