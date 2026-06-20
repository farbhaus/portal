<script lang="ts">
  import { invalidateAll } from "$app/navigation";
  import { Button, StatusPill } from "$lib/components";
  import type { Destination, SyncRule } from "./+page.server";

  let { data } = $props();
  const destinations = $derived(data.destinations as Destination[]);
  const rules = $derived(data.rules as SyncRule[]);

  let busy = $state<string | null>(null);
  let note = $state<string | null>(null);
  let deleting = $state<string | null>(null);

  function sourceLabel(r: SyncRule): string {
    const s = r.source as Record<string, unknown>;
    const name = (s.folder_name as string) || "folder";
    return (s.recursive ?? true) ? `${name} (recursive)` : name;
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
      note = res.ok ? `Reconcile queued for "${r.name}".` : `Could not run (${res.status}).`;
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

<div class="space-y-5">
  <div class="flex flex-wrap items-start justify-between gap-4">
    <div class="min-w-0">
      <h1 class="text-xl font-semibold tracking-tight">Sync</h1>
      <p class="mt-0.5 text-sm text-muted">Destinations and rules for syncing Frame.io to local storage.</p>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <Button href="/destinations/new" variant="ghost">New destination</Button>
      <Button href="/sync-rules/new">New rule</Button>
    </div>
  </div>

  {#if note}<p class="rounded-md bg-surface-2 px-3 py-2 text-sm text-muted">{note}</p>{/if}

  <div class="grid gap-5 lg:grid-cols-2">
    <!-- Destinations -->
    <div class="rounded-card border border-border bg-surface shadow-sm">
      <div class="flex items-center justify-between border-b border-border px-5 py-3.5">
        <div>
          <h2 class="text-sm font-semibold">Destinations</h2>
          <p class="text-xs text-muted">Frame.io folders upload links deliver into.</p>
        </div>
        <span class="text-xs text-faint">{destinations.length}</span>
      </div>

      {#if destinations.length === 0}
        <div class="px-5 py-10 text-center">
          <p class="text-sm text-muted">No destinations yet.</p>
          <Button href="/destinations/new" size="sm" class="mt-3">Create one</Button>
        </div>
      {:else}
        <div class="divide-y divide-border/60">
          {#each destinations as dest (dest.id)}
            <div class="group px-5 py-3 hover:bg-surface-2 transition-colors">
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    {#if dest.accent_color}
                      <span class="h-2.5 w-2.5 shrink-0 rounded-full border border-border/60" style="background:{dest.accent_color}"></span>
                    {/if}
                    <span class="truncate font-medium text-sm">{dest.display_name}</span>
                  </div>
                  {#if dest.subtitle}<div class="mt-0.5 truncate text-xs text-muted">{dest.subtitle}</div>{/if}
                  <div class="mt-0.5 truncate text-xs text-faint">{dest.config.folder_name ?? dest.config.folder_id}</div>
                </div>
                <div class="flex shrink-0 items-center gap-2 opacity-100 transition-opacity sm:opacity-0 sm:group-hover:opacity-100">
                  <a href="/destinations/{dest.id}" class="text-xs text-muted hover:text-text">Edit</a>
                  <button onclick={() => removeDestination(dest.id, dest.display_name)} disabled={deleting === dest.id} class="text-xs text-danger hover:opacity-80 disabled:opacity-50">
                    {deleting === dest.id ? "Deleting…" : "Delete"}
                  </button>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Sync rules -->
    <div class="rounded-card border border-border bg-surface shadow-sm">
      <div class="flex items-center justify-between border-b border-border px-5 py-3.5">
        <div>
          <h2 class="text-sm font-semibold">Sync rules</h2>
          <p class="text-xs text-muted">Mirror a Frame.io folder to a local path.</p>
        </div>
        <span class="text-xs text-faint">{rules.length}</span>
      </div>

      {#if rules.length === 0}
        <div class="px-5 py-10 text-center">
          <p class="text-sm text-muted">No sync rules yet.</p>
          <Button href="/sync-rules/new" size="sm" class="mt-3">Create one</Button>
        </div>
      {:else}
        <div class="divide-y divide-border/60">
          {#each rules as r (r.id)}
            <div class="group px-5 py-3 hover:bg-surface-2 transition-colors">
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <span class="truncate font-medium text-sm">{r.name}</span>
                    {#if r.enabled}
                      <StatusPill status="ok" label={r.subscription_active ? "enabled" : "enabled (no webhook)"} />
                    {:else}
                      <StatusPill status="revoked" label="disabled" />
                    {/if}
                  </div>
                  <div class="mt-0.5 truncate text-xs text-muted">{sourceLabel(r)}</div>
                  <div class="mt-0.5 truncate font-mono text-xs text-faint">{r.destination_path}</div>
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
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>
