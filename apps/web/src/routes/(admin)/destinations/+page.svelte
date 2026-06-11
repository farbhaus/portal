<script lang="ts">
  import { invalidateAll } from "$app/navigation";
  import { PageHeader, Tabs, Button, Table, EmptyState } from "$lib/components";

  let { data } = $props();
  let deleting = $state<string | null>(null);

  const syncTabs = [
    { label: "Destinations", href: "/destinations" },
    { label: "Sync rules", href: "/sync-rules" },
  ];

  async function remove(id: string, name: string) {
    if (!confirm(`Delete destination "${name}"? Upload links using it will also be removed.`)) return;
    deleting = id;
    try {
      await fetch(`/api/destinations/${id}`, { method: "DELETE" });
      await invalidateAll();
    } finally {
      deleting = null;
    }
  }
</script>

<div class="space-y-5">
  <PageHeader title="Sync" subtitle="Frame.io folders that upload links deliver into.">
    {#snippet actions()}
      <Button href="/destinations/new">New destination</Button>
    {/snippet}
  </PageHeader>

  <Tabs tabs={syncTabs} />

  {#if data.destinations.length === 0}
    <EmptyState message="No destinations yet.">
      <Button href="/destinations/new" size="sm">Create your first one</Button>
    </EmptyState>
  {:else}
    <Table columns={["Name", "Frame.io folder", "Accent", { label: "", class: "text-right" }]}>
      {#each data.destinations as dest (dest.id)}
        <tr>
          <td class="px-4 py-3">
            <div class="font-medium">{dest.display_name}</div>
            {#if dest.subtitle}<div class="text-xs text-muted">{dest.subtitle}</div>{/if}
          </td>
          <td class="px-4 py-3 text-muted">{dest.config.folder_name ?? dest.config.folder_id}</td>
          <td class="px-4 py-3">
            {#if dest.accent_color}
              <span class="inline-flex items-center gap-1.5">
                <span class="h-3.5 w-3.5 rounded-full border border-border" style="background:{dest.accent_color}"></span>
                <span class="text-xs text-muted">{dest.accent_color}</span>
              </span>
            {:else}
              <span class="text-xs text-faint">—</span>
            {/if}
          </td>
          <td class="px-4 py-3 text-right whitespace-nowrap">
            <a href="/destinations/{dest.id}" class="text-muted hover:text-text">Edit</a>
            <button onclick={() => remove(dest.id, dest.display_name)} disabled={deleting === dest.id} class="ml-3 text-danger hover:opacity-80 disabled:opacity-50">
              {deleting === dest.id ? "Deleting…" : "Delete"}
            </button>
          </td>
        </tr>
      {/each}
    </Table>
  {/if}
</div>
