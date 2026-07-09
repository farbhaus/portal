<script lang="ts">
  import { goto, invalidateAll } from "$app/navigation";
  import { PathBreadcrumb, TemplateTokenInput } from "$lib/components";
  import { SYNC_TOKENS } from "$lib/tokens";
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
  let runNote = $state<string | null>(null);

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
    done: "bg-success/10 text-success",
    pending: "bg-warning/10 text-warning",
    running: "bg-info/10 text-info",
    waiting: "bg-purple-50 text-purple-700",
    skipped: "bg-surface-2 text-muted",
    dead_letter: "bg-danger/10 text-danger",
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
    error = null;
    saved = false;
    runNote = null;
    const res = await fetch(`/api/sync-rules/${rule.id}/run`, { method: "POST" });
    if (!res.ok) {
      error = `Could not run (${res.status}).`;
      return;
    }
    const created = (await res.json().catch(() => ({}))).created ?? 0;
    runNote = created > 0
      ? `Queued ${created} file${created === 1 ? "" : "s"}.`
      : "Already up to date — nothing new to sync.";
    await invalidateAll();
  }

  async function remove() {
    if (!confirm("Delete this rule and its webhook?")) return;
    await fetch(`/api/sync-rules/${rule.id}`, { method: "DELETE" });
    await goto("/sync-rules");
  }
</script>

<div class="mx-auto max-w-3xl space-y-6">
  <div>
    <a href="/sync-rules" class="text-sm text-muted hover:text-text">← Destinations</a>
    <h1 class="mt-1 text-2xl font-semibold">{rule.name}</h1>
  </div>

  {#if error}<p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
  {:else if saved}<p class="rounded-md bg-success/10 px-3 py-2 text-sm text-success">Saved.</p>
  {:else if runNote}<p class="rounded-md bg-surface-2 px-3 py-2 text-sm text-muted">{runNote}</p>{/if}

  <div class="rounded-xl border border-border bg-surface p-6 text-sm">
    <div class="flex items-center justify-between gap-3">
      <span class="shrink-0 text-muted">Source</span>
      {#if rule.source.path?.length}
        <span class="min-w-0 text-right">
          <PathBreadcrumb segments={rule.source.path} class="text-sm" />
          {#if rule.source.recursive ?? true}<span class="text-xs text-faint"> (recursive)</span>{/if}
        </span>
      {:else}
        <span>{sourceLabel()}</span>
      {/if}
    </div>
    <div class="mt-2 flex items-center justify-between">
      <span class="text-muted">Webhook</span>
      <span>{rule.enabled ? (rule.subscription_active ? "active" : "missing") : "disabled"}</span>
    </div>
    <div class="mt-3 flex items-center gap-2">
      <button onclick={toggle} class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-2">{rule.enabled ? "Disable" : "Enable"}</button>
      <button onclick={runNow} class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-2">Run now</button>
    </div>
    <p class="mt-2 text-xs text-faint">The source folder is fixed. Create a new rule to watch a different folder.</p>
  </div>

  <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Settings</h2>
    <label class="block text-sm">
      <span class="text-muted">Rule name</span>
      <input bind:value={name} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-muted">Destination path</span>
      <input bind:value={destinationPath} class="mt-1 w-full rounded-md border border-border px-2 py-1.5 font-mono" />
    </label>
    <label class="block text-sm">
      <span class="text-muted">On filename conflict</span>
      <select bind:value={conflictPolicy} class="mt-1 w-full rounded-md border border-border px-2 py-1.5">
        <option value="rename_suffix">Rename (add suffix)</option>
        <option value="skip">Skip</option>
        <option value="overwrite">Overwrite</option>
      </select>
    </label>
    <div class="block text-sm">
      <span class="text-muted">Path template (optional)</span>
      <TemplateTokenInput bind:value={pathTemplate} tokens={SYNC_TOKENS} placeholder={"{project}/{date}/{filename}"} />
      <p class="mt-1 text-xs text-faint">Leave blank to keep original names at the destination root.</p>
    </div>
    <div class="rounded-md border border-border bg-surface-2 p-3 text-xs text-muted">
      <div class="mb-1 font-medium text-faint">Examples</div>
      <ul class="space-y-0.5 font-mono">
        <li>{"{project}/{date}/{filename}"} → Acme/2026-06-12/clip.mov</li>
        <li>{"{year}/{month}/{filename}"} → 2026/06/clip.mov</li>
        <li>{"{folder}/{stem}{ext}"} → Dailies/clip.mov</li>
      </ul>
    </div>
    <div class="flex items-center justify-between">
      <button onclick={save} disabled={saving} class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50">{saving ? "Saving…" : "Save changes"}</button>
      <button onclick={remove} class="text-sm text-danger hover:text-danger">Delete</button>
    </div>
  </div>

  <div class="space-y-3 rounded-xl border border-border bg-surface p-6">
    <div class="flex items-center justify-between">
      <h2 class="font-medium">Jobs</h2>
      <div class="flex gap-2 text-xs text-muted">
        {#each Object.entries(stats) as [s, n] (s)}<span>{s}: {n}</span>{/each}
      </div>
    </div>
    {#if jobs.length === 0}
      <p class="text-sm text-faint">No jobs yet. New files in the source folder will appear here.</p>
    {:else}
      <div class="overflow-hidden rounded-lg border border-border">
        <table class="w-full text-sm">
          <thead class="border-b border-border text-left text-muted">
            <tr>
              <th class="px-3 py-2 font-medium">File</th>
              <th class="px-3 py-2 font-medium">Status</th>
              <th class="px-3 py-2 font-medium">Size</th>
              <th class="px-3 py-2 font-medium">When</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-border">
            {#each jobs as j (j.id)}
              <tr>
                <td class="px-3 py-2 font-mono text-xs">{j.frameio_file_id}</td>
                <td class="px-3 py-2">
                  <span class="rounded-full px-2 py-0.5 text-xs {statusColor[j.status] ?? 'bg-surface-2 text-muted'}">{j.status}</span>
                  {#if j.retry_count > 0}<span class="ml-1 text-xs text-faint">×{j.retry_count}</span>{/if}
                </td>
                <td class="px-3 py-2 text-muted">{fmtBytes(j.bytes)}</td>
                <td class="px-3 py-2 text-xs text-faint">{new Date(j.created_at).toLocaleString()}</td>
              </tr>
              {#if j.error}
                <tr><td colspan="4" class="px-3 pb-2 text-xs text-danger">{j.error}</td></tr>
              {/if}
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</div>
