<script lang="ts">
  import { invalidateAll } from "$app/navigation";
  import { PageHeader, Tabs, Button, Table, StatusPill, EmptyState } from "$lib/components";
  import type { SyncRule } from "./+page.server";

  let { data } = $props();
  const rules = $derived(data.rules as SyncRule[]);

  let busy = $state<string | null>(null);
  let note = $state<string | null>(null);

  const syncTabs = [
    { label: "Destinations", href: "/destinations" },
    { label: "Sync rules", href: "/sync-rules" },
  ];

  function sourceLabel(r: SyncRule): string {
    const s = r.source as Record<string, unknown>;
    const name = (s.folder_name as string) || "folder";
    return (s.recursive ?? true) ? `${name} (recursive)` : name;
  }

  async function run(r: SyncRule) {
    busy = r.id;
    note = null;
    try {
      const res = await fetch(`/api/sync-rules/${r.id}/run`, { method: "POST" });
      note = res.ok ? `Reconcile queued for “${r.name}”.` : `Could not run (${res.status}).`;
    } finally {
      busy = null;
    }
  }

  async function toggle(r: SyncRule) {
    busy = r.id;
    note = null;
    try {
      const res = await fetch(`/api/sync-rules/${r.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ enabled: !r.enabled }),
      });
      if (!res.ok) note = (await res.json().catch(() => ({}))).detail ?? `Could not update (${res.status}).`;
      else await invalidateAll();
    } finally {
      busy = null;
    }
  }
</script>

<div class="space-y-5">
  <PageHeader title="Sync" subtitle="Mirror a Frame.io folder to a local path automatically.">
    {#snippet actions()}
      <Button href="/sync-rules/new">New sync rule</Button>
    {/snippet}
  </PageHeader>

  <Tabs tabs={syncTabs} />

  {#if note}<p class="rounded-md bg-surface-2 px-3 py-2 text-sm text-muted">{note}</p>{/if}

  {#if rules.length === 0}
    <EmptyState message="No sync rules yet. Create one to mirror a Frame.io folder to a local path.">
      <Button href="/sync-rules/new" size="sm">Create one</Button>
    </EmptyState>
  {:else}
    <Table columns={["Name", "Source", "Destination", "Status", { label: "", class: "text-right" }]}>
      {#each rules as r (r.id)}
        <tr>
          <td class="px-4 py-2.5"><a href="/sync-rules/{r.id}" class="font-medium hover:text-accent">{r.name}</a></td>
          <td class="px-4 py-2.5 text-muted">{sourceLabel(r)}</td>
          <td class="px-4 py-2.5 font-mono text-xs text-muted">{r.destination_path}</td>
          <td class="px-4 py-2.5">
            {#if r.enabled}
              <StatusPill status="ok" label={r.subscription_active ? "enabled" : "enabled (no webhook)"} />
            {:else}
              <StatusPill status="revoked" label="disabled" />
            {/if}
          </td>
          <td class="px-4 py-2.5 text-right whitespace-nowrap">
            <Button variant="ghost" size="sm" onclick={() => run(r)} disabled={busy === r.id}>Run now</Button>
            <span class="ml-1 inline-block">
              <Button variant="ghost" size="sm" onclick={() => toggle(r)} disabled={busy === r.id}>{r.enabled ? "Disable" : "Enable"}</Button>
            </span>
          </td>
        </tr>
      {/each}
    </Table>
  {/if}
</div>
