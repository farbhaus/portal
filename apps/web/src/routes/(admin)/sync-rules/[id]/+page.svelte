<script lang="ts">
  import { goto, invalidateAll } from "$app/navigation";
  import type { SyncJob } from "./+page.server";

  let { data } = $props();
  const rule = $derived(data.rule);
  const jobs = $derived(data.jobs as SyncJob[]);
  const stats = $derived(data.stats.counts as Record<string, number>);

  let name = $state("");
  let destinationPath = $state("");
  let conflictPolicy = $state("rename_suffix");
  let pathTemplate = $state("");

  let saving = $state(false);
  let saved = $state(false);
  let error = $state<string | null>(null);

  let seededId = "";
  $effect(() => {
    if (rule.id !== seededId) {
      seededId = rule.id;
      name = rule.name;
      destinationPath = rule.destination_path;
      conflictPolicy = rule.conflict_policy;
      pathTemplate = rule.path_template ?? "";
    }
  });

  function sourceLabel(): string {
    const s = rule.source as Record<string, unknown>;
    const folder = (s.folder_name as string) || "folder";
    const proj = (s.project_name as string) || "";
    const rec = (s.recursive ?? true) ? " (recursive)" : "";
    return proj ? `${proj} / ${folder}${rec}` : `${folder}${rec}`;
  }

  function fmtBytes(n: number | null): string {
    if (n === null) return "";
    if (n < 1024) return `${n} B`;
    const u = ["KB", "MB", "GB", "TB"];
    let v = n / 1024,
      i = 0;
    while (v >= 1024 && i < u.length - 1) {
      v /= 1024;
      i++;
    }
    return `${v.toFixed(1)} ${u[i]}`;
  }

  const statusColor: Record<string, string> = {
    done: "bg-green-50 text-green-700",
    pending: "bg-amber-50 text-amber-700",
    running: "bg-blue-50 text-blue-700",
    skipped: "bg-neutral-100 text-neutral-500",
    dead_letter: "bg-red-50 text-red-700",
  };

  async function save() {
    saving = true;
    error = null;
    saved = false;
    try {
      const res = await fetch(`/api/sync-rules/${rule.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          name: name.trim(),
          destination_path: destinationPath.trim(),
          conflict_policy: conflictPolicy,
          path_template: pathTemplate.trim() || null,
        }),
      });
      if (!res.ok) {
        error = (await res.json().catch(() => ({}))).detail ?? `Could not save (${res.status})`;
        return;
      }
      saved = true;
      await invalidateAll();
    } finally {
      saving = false;
    }
  }

  async function toggle() {
    error = null;
    const res = await fetch(`/api/sync-rules/${rule.id}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled: !rule.enabled }),
    });
    if (!res.ok) error = (await res.json().catch(() => ({}))).detail ?? `Could not update (${res.status})`;
    else await invalidateAll();
  }

  async function runNow() {
    await fetch(`/api/sync-rules/${rule.id}/run`, { method: "POST" });
    setTimeout(invalidateAll, 1500);
  }

  async function remove() {
    if (!confirm("Delete this rule and its webhook?")) return;
    await fetch(`/api/sync-rules/${rule.id}`, { method: "DELETE" });
    await goto("/sync-rules");
  }
</script>

<div class="max-w-3xl space-y-6">
  <div>
    <a href="/sync-rules" class="text-sm text-neutral-500 hover:text-neutral-900">← Sync rules</a>
    <h1 class="mt-1 text-2xl font-semibold">{rule.name}</h1>
  </div>

  {#if error}<p class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">{error}</p>
  {:else if saved}<p class="rounded-md bg-green-50 px-3 py-2 text-sm text-green-700">Saved.</p>{/if}

  <div class="rounded-xl border border-neutral-200 bg-white p-6 text-sm">
    <div class="flex items-center justify-between">
      <span class="text-neutral-500">Source</span><span>{sourceLabel()}</span>
    </div>
    <div class="mt-2 flex items-center justify-between">
      <span class="text-neutral-500">Webhook</span>
      <span>{rule.enabled ? (rule.subscription_active ? "active" : "missing") : "disabled"}</span>
    </div>
    <div class="mt-3 flex items-center gap-2">
      <button onclick={toggle} class="rounded-md border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100">{rule.enabled ? "Disable" : "Enable"}</button>
      <button onclick={runNow} class="rounded-md border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100">Run now</button>
    </div>
    <p class="mt-2 text-xs text-neutral-400">The source folder is fixed. Create a new rule to watch a different folder.</p>
  </div>

  <div class="space-y-4 rounded-xl border border-neutral-200 bg-white p-6">
    <h2 class="font-medium">Settings</h2>
    <label class="block text-sm">
      <span class="text-neutral-500">Rule name</span>
      <input bind:value={name} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-neutral-500">Destination path</span>
      <input bind:value={destinationPath} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5 font-mono" />
    </label>
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <label class="block text-sm">
        <span class="text-neutral-500">On filename conflict</span>
        <select bind:value={conflictPolicy} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5">
          <option value="rename_suffix">Rename (add suffix)</option>
          <option value="skip">Skip</option>
          <option value="overwrite">Overwrite</option>
        </select>
      </label>
      <label class="block text-sm">
        <span class="text-neutral-500">Path template</span>
        <input bind:value={pathTemplate} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5 font-mono" />
      </label>
    </div>
    <div class="flex items-center justify-between">
      <button onclick={save} disabled={saving} class="rounded-md bg-neutral-900 px-4 py-2 text-sm font-medium text-white hover:bg-neutral-800 disabled:opacity-50">{saving ? "Saving…" : "Save changes"}</button>
      <button onclick={remove} class="text-sm text-red-600 hover:text-red-700">Delete</button>
    </div>
  </div>

  <div class="space-y-3 rounded-xl border border-neutral-200 bg-white p-6">
    <div class="flex items-center justify-between">
      <h2 class="font-medium">Jobs</h2>
      <div class="flex gap-2 text-xs text-neutral-500">
        {#each Object.entries(stats) as [s, n] (s)}<span>{s}: {n}</span>{/each}
      </div>
    </div>
    {#if jobs.length === 0}
      <p class="text-sm text-neutral-400">No jobs yet. New files in the source folder will appear here.</p>
    {:else}
      <div class="overflow-hidden rounded-lg border border-neutral-100">
        <table class="w-full text-sm">
          <thead class="border-b border-neutral-100 text-left text-neutral-500">
            <tr>
              <th class="px-3 py-2 font-medium">File</th>
              <th class="px-3 py-2 font-medium">Status</th>
              <th class="px-3 py-2 font-medium">Size</th>
              <th class="px-3 py-2 font-medium">When</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-neutral-50">
            {#each jobs as j (j.id)}
              <tr>
                <td class="px-3 py-2 font-mono text-xs">{j.frameio_file_id}</td>
                <td class="px-3 py-2">
                  <span class="rounded-full px-2 py-0.5 text-xs {statusColor[j.status] ?? 'bg-neutral-100 text-neutral-500'}">{j.status}</span>
                  {#if j.retry_count > 0}<span class="ml-1 text-xs text-neutral-400">×{j.retry_count}</span>{/if}
                </td>
                <td class="px-3 py-2 text-neutral-500">{fmtBytes(j.bytes)}</td>
                <td class="px-3 py-2 text-xs text-neutral-400">{new Date(j.created_at).toLocaleString()}</td>
              </tr>
              {#if j.error}
                <tr><td colspan="4" class="px-3 pb-2 text-xs text-red-600">{j.error}</td></tr>
              {/if}
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</div>
