<script lang="ts">
  import { invalidateAll } from "$app/navigation";
  import { Button, PathBreadcrumb, StatusPill } from "$lib/components";
  import type { Destination, SyncRule } from "./+page.server";

  let { data } = $props();
  const destinations = $derived(data.destinations as Destination[]);
  const rules = $derived(data.rules as SyncRule[]);

  let busy = $state<string | null>(null);
  let note = $state<string | null>(null);
  let deleting = $state<string | null>(null);

  const ruleById = $derived(new Map(rules.map((r) => [r.id, r])));
  // Rules not covering any destination still need a home on this page.
  const otherRules = $derived.by(() => {
    const covered = new Set(destinations.flatMap((d) => d.sync_rules.map((sr) => sr.id)));
    return rules.filter((r) => !covered.has(r.id));
  });

  function destPath(dest: Destination): { id?: string; name: string }[] {
    if (dest.config.path?.length) return dest.config.path;
    return [{ name: dest.config.folder_name ?? dest.config.folder_id ?? "folder" }];
  }
  function rulePath(r: SyncRule): { id?: string; name: string }[] {
    if (r.source.path?.length) return r.source.path;
    const segs: { name: string }[] = [];
    if (r.source.project_name) segs.push({ name: r.source.project_name });
    segs.push({ name: r.source.folder_name ?? r.source.folder_id ?? "folder" });
    return segs;
  }

  async function removeDestination(id: string, name: string) {
    if (!confirm(`Delete destination "${name}"? Upload links using it will also be removed.`)) return;
    deleting = id;
    try {
      await fetch(`/api/destinations/${id}`, { method: "DELETE" });
      await invalidateAll();
    } finally { deleting = null; }
  }

  async function run(r: SyncRule) {
    busy = r.id; note = null;
    try {
      const res = await fetch(`/api/sync-rules/${r.id}/run`, { method: "POST" });
      if (!res.ok) {
        note = `Could not run (${res.status}).`;
      } else {
        const created = (await res.json().catch(() => ({}))).created ?? 0;
        note = created > 0
          ? `Queued ${created} file${created === 1 ? "" : "s"} for "${r.name}".`
          : `"${r.name}" is already up to date — nothing new to sync.`;
      }
    } finally { busy = null; }
  }

  async function toggle(r: SyncRule) {
    busy = r.id; note = null;
    try {
      const res = await fetch(`/api/sync-rules/${r.id}`, {
        method: "PATCH", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ enabled: !r.enabled }),
      });
      if (!res.ok) note = (await res.json().catch(() => ({}))).detail ?? `Could not update (${res.status}).`;
      else await invalidateAll();
    } finally { busy = null; }
  }

  async function removeRule(r: SyncRule) {
    if (!confirm(`Delete sync rule "${r.name}"? Its webhook and job history will be removed.`)) return;
    deleting = r.id; note = null;
    try {
      const res = await fetch(`/api/sync-rules/${r.id}`, { method: "DELETE" });
      if (!res.ok) note = (await res.json().catch(() => ({}))).detail ?? `Could not delete (${res.status}).`;
      else await invalidateAll();
    } finally { deleting = null; }
  }
</script>

{#snippet ruleRow(r: SyncRule, exact: boolean | null, showSource: boolean)}
  <div class="group flex items-start justify-between gap-3 px-4 py-2.5 hover:bg-surface-2 transition-colors">
    <div class="min-w-0 flex-1">
      <div class="flex flex-wrap items-center gap-2">
        <a href="/sync-rules/{r.id}" class="truncate text-sm font-medium hover:text-accent">{r.name}</a>
        {#if r.enabled}
          <StatusPill status="ok" label={r.subscription_active ? "enabled" : "enabled (no webhook)"} />
        {:else}
          <StatusPill status="revoked" label="disabled" />
        {/if}
        {#if exact === false}
          <span class="text-xs text-faint">via parent folder</span>
        {/if}
      </div>
      {#if showSource}
        <div class="mt-0.5 text-xs">
          <PathBreadcrumb segments={rulePath(r)} class="text-xs" />
          {#if r.source.recursive ?? true}<span class="text-faint"> (recursive)</span>{/if}
        </div>
      {/if}
      <div class="mt-0.5 truncate text-xs">
        <span class="text-faint">→</span>
        <PathBreadcrumb segments={[{ name: r.destination_path }]} variant="local" class="text-xs" />
      </div>
    </div>
    <div class="flex shrink-0 flex-wrap items-center justify-end gap-1.5 opacity-100 transition-opacity sm:opacity-0 sm:group-hover:opacity-100">
      <Button variant="ghost" size="sm" onclick={() => run(r)} disabled={busy === r.id}>Run</Button>
      <Button variant="ghost" size="sm" onclick={() => toggle(r)} disabled={busy === r.id}>
        {r.enabled ? "Disable" : "Enable"}
      </Button>
      <a href="/sync-rules/{r.id}" class="text-xs text-muted hover:text-text">Edit</a>
      <button onclick={() => removeRule(r)} disabled={deleting === r.id} class="text-xs text-danger hover:opacity-80 disabled:opacity-50">
        {deleting === r.id ? "Deleting…" : "Delete"}
      </button>
    </div>
  </div>
{/snippet}

<div class="space-y-5">
  <div class="flex flex-wrap items-start justify-between gap-4">
    <div class="min-w-0">
      <h1 class="text-xl font-semibold tracking-tight">Destinations</h1>
      <p class="mt-0.5 text-sm text-muted">Where files land in Frame.io — with the links and sync rules attached to each folder.</p>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <Button href="/sync-rules/new" variant="ghost">New sync rule</Button>
      <Button href="/destinations/new">New destination</Button>
    </div>
  </div>

  {#if note}<p class="rounded-md bg-surface-2 px-3 py-2 text-sm text-muted">{note}</p>{/if}

  {#if destinations.length === 0}
    <div class="rounded-card border border-border bg-surface px-5 py-12 text-center shadow-sm">
      <p class="text-sm text-muted">No destinations yet. A destination is the Frame.io folder your upload links deliver into.</p>
      <Button href="/destinations/new" size="sm" class="mt-3">Create one</Button>
    </div>
  {/if}

  {#each destinations as dest (dest.id)}
    <div class="rounded-card border border-border bg-surface shadow-sm">
      <div class="flex flex-wrap items-start justify-between gap-3 border-b border-border px-5 py-3.5">
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-2">
            <a href="/destinations/{dest.id}" class="truncate font-semibold text-sm hover:text-accent">{dest.display_name}</a>
            {#if dest.sync_rules.some((sr) => sr.enabled)}
              <StatusPill status="ok" label="synced" />
            {:else}
              <StatusPill status="revoked" label="not synced" />
            {/if}
          </div>
          {#if dest.subtitle}<div class="mt-0.5 truncate text-xs text-muted">{dest.subtitle}</div>{/if}
          <div class="mt-1 text-xs">
            <PathBreadcrumb segments={destPath(dest)} class="text-xs" />
          </div>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <a href="/destinations/{dest.id}" class="text-xs text-muted hover:text-text">Edit</a>
          <button onclick={() => removeDestination(dest.id, dest.display_name)} disabled={deleting === dest.id} class="text-xs text-danger hover:opacity-80 disabled:opacity-50">
            {deleting === dest.id ? "Deleting…" : "Delete"}
          </button>
        </div>
      </div>

      <div class="grid divide-y divide-border/60 lg:grid-cols-2 lg:divide-x lg:divide-y-0">
        <!-- Upload links into this folder -->
        <div class="py-1.5">
          <div class="flex items-center justify-between px-4 pb-1 pt-1.5">
            <span class="text-xs font-medium uppercase tracking-wide text-faint">Upload links</span>
            <a href="/links/new?type=upload&destination={dest.id}" class="text-xs text-accent hover:underline">+ Upload link</a>
          </div>
          {#if dest.upload_links.length === 0}
            <p class="px-4 py-2 text-xs text-faint">No upload links deliver here yet.</p>
          {:else}
            {#each dest.upload_links as link (link.id)}
              <div class="flex items-center justify-between gap-3 px-4 py-1.5 hover:bg-surface-2 transition-colors">
                <a href="/upload-links/{link.id}" class="truncate text-sm hover:text-accent">{link.label}</a>
                <StatusPill status={link.state} />
              </div>
            {/each}
          {/if}
        </div>

        <!-- Sync coverage -->
        <div class="py-1.5">
          <div class="flex items-center justify-between px-4 pb-1 pt-1.5">
            <span class="text-xs font-medium uppercase tracking-wide text-faint">Sync to local storage</span>
            <a href="/sync-rules/new?destination={dest.id}" class="text-xs text-accent hover:underline">+ Sync rule</a>
          </div>
          {#if dest.sync_rules.length === 0}
            <p class="px-4 py-2 text-xs text-faint">Not synced — files stay in Frame.io only.</p>
          {:else}
            {#each dest.sync_rules as sr (sr.id)}
              {@const r = ruleById.get(sr.id)}
              {#if r}{@render ruleRow(r, sr.exact, false)}{/if}
            {/each}
          {/if}
        </div>
      </div>
    </div>
  {/each}

  {#if otherRules.length > 0}
    <div class="rounded-card border border-border bg-surface shadow-sm">
      <div class="border-b border-border px-5 py-3.5">
        <h2 class="text-sm font-semibold">Other sync rules</h2>
        <p class="text-xs text-muted">Watching Frame.io folders that aren't a destination.</p>
      </div>
      <div class="divide-y divide-border/60">
        {#each otherRules as r (r.id)}
          {@render ruleRow(r, null, true)}
        {/each}
      </div>
    </div>
  {/if}
</div>
