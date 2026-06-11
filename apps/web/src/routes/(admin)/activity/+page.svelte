<script lang="ts">
  import { goto } from "$app/navigation";
  import { PageHeader, Table, EmptyState } from "$lib/components";
  import type { Activity } from "./+page.server";

  let { data } = $props();
  const events = $derived(data.events as Activity[]);

  function onFilter(e: Event) {
    const v = (e.target as HTMLSelectElement).value;
    goto(v ? `/activity?action=${encodeURIComponent(v)}` : "/activity");
  }

  function detailText(d: Record<string, unknown> | null): string {
    if (!d) return "";
    return Object.entries(d)
      .map(([k, v]) => `${k}=${v}`)
      .join("  ");
  }
</script>

<div class="space-y-5">
  <PageHeader title="Activity" subtitle="Append-only log of admin actions.">
    {#snippet actions()}
      <select onchange={onFilter} class="rounded-md border border-border bg-surface-2 px-2 py-1.5 text-sm">
        <option value="" selected={!data.action}>All actions</option>
        {#each data.actions as a (a)}
          <option value={a} selected={data.action === a}>{a}</option>
        {/each}
      </select>
    {/snippet}
  </PageHeader>

  {#if events.length === 0}
    <EmptyState message="No activity yet." />
  {:else}
    <Table columns={["When", "Who", "Action", "Detail"]}>
      {#each events as ev (ev.id)}
        <tr>
          <td class="px-4 py-2.5 whitespace-nowrap text-muted">{new Date(ev.created_at).toLocaleString()}</td>
          <td class="px-4 py-2.5 text-muted">{ev.user_email ?? "—"}</td>
          <td class="px-4 py-2.5"><span class="rounded-full bg-surface-3 px-2 py-0.5 text-xs">{ev.action}</span></td>
          <td class="px-4 py-2.5 font-mono text-xs text-faint">{detailText(ev.detail)}</td>
        </tr>
      {/each}
    </Table>
  {/if}
</div>
