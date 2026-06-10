<script lang="ts">
  import { invalidateAll } from "$app/navigation";
  import type { SyncRule } from "./+page.server";

  let { data } = $props();
  const rules = $derived(data.rules as SyncRule[]);

  let busy = $state<string | null>(null);
  let note = $state<string | null>(null);

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

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-semibold">Sync rules</h1>
    <a href="/sync-rules/new" class="rounded-md bg-neutral-900 px-4 py-2 text-sm font-medium text-white hover:bg-neutral-800">New rule</a>
  </div>

  {#if note}<p class="rounded-md bg-neutral-100 px-3 py-2 text-sm text-neutral-700">{note}</p>{/if}

  {#if rules.length === 0}
    <div class="rounded-xl border border-dashed border-neutral-300 p-10 text-center text-neutral-500">
      No sync rules yet. Create one to mirror a Frame.io folder to a local path.
    </div>
  {:else}
    <div class="overflow-hidden rounded-xl border border-neutral-200 bg-white">
      <table class="w-full text-sm">
        <thead class="border-b border-neutral-200 text-left text-neutral-500">
          <tr>
            <th class="px-4 py-2 font-medium">Name</th>
            <th class="px-4 py-2 font-medium">Source</th>
            <th class="px-4 py-2 font-medium">Destination</th>
            <th class="px-4 py-2 font-medium">Status</th>
            <th class="px-4 py-2"></th>
          </tr>
        </thead>
        <tbody class="divide-y divide-neutral-100">
          {#each rules as r (r.id)}
            <tr>
              <td class="px-4 py-2"><a href="/sync-rules/{r.id}" class="font-medium hover:underline">{r.name}</a></td>
              <td class="px-4 py-2 text-neutral-500">{sourceLabel(r)}</td>
              <td class="px-4 py-2 font-mono text-xs text-neutral-500">{r.destination_path}</td>
              <td class="px-4 py-2">
                {#if r.enabled}
                  <span class="rounded-full bg-green-50 px-2 py-0.5 text-xs text-green-700">enabled{r.subscription_active ? "" : " (no webhook)"}</span>
                {:else}
                  <span class="rounded-full bg-neutral-100 px-2 py-0.5 text-xs text-neutral-500">disabled</span>
                {/if}
              </td>
              <td class="px-4 py-2 text-right whitespace-nowrap">
                <button onclick={() => run(r)} disabled={busy === r.id} class="rounded-md border border-neutral-300 px-2 py-1 text-xs hover:bg-neutral-100 disabled:opacity-50">Run now</button>
                <button onclick={() => toggle(r)} disabled={busy === r.id} class="ml-1 rounded-md border border-neutral-300 px-2 py-1 text-xs hover:bg-neutral-100 disabled:opacity-50">{r.enabled ? "Disable" : "Enable"}</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
