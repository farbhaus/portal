<script lang="ts">
  import { invalidateAll } from "$app/navigation";

  let { data } = $props();
  let deleting = $state<string | null>(null);

  async function remove(id: string, name: string) {
    if (!confirm(`Delete destination "${name}"? Upload links using it will also be removed.`))
      return;
    deleting = id;
    try {
      await fetch(`/api/destinations/${id}`, { method: "DELETE" });
      await invalidateAll();
    } finally {
      deleting = null;
    }
  }
</script>

<div class="max-w-4xl space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold">Destinations</h1>
      <p class="text-sm text-neutral-500">Frame.io folders that upload links deliver into.</p>
    </div>
    <a
      href="/destinations/new"
      class="rounded-md bg-neutral-900 px-4 py-2 text-sm font-medium text-white hover:bg-neutral-800"
    >
      New destination
    </a>
  </div>

  {#if data.destinations.length === 0}
    <div class="rounded-xl border border-dashed border-neutral-300 bg-white p-10 text-center">
      <p class="text-sm text-neutral-500">No destinations yet.</p>
      <a href="/destinations/new" class="mt-2 inline-block text-sm font-medium underline">
        Create your first one
      </a>
    </div>
  {:else}
    <div class="overflow-hidden rounded-xl border border-neutral-200 bg-white">
      <table class="w-full text-sm">
        <thead class="border-b border-neutral-200 text-left text-neutral-500">
          <tr>
            <th class="px-4 py-2.5 font-medium">Name</th>
            <th class="px-4 py-2.5 font-medium">Frame.io folder</th>
            <th class="px-4 py-2.5 font-medium">Accent</th>
            <th class="px-4 py-2.5"></th>
          </tr>
        </thead>
        <tbody>
          {#each data.destinations as dest (dest.id)}
            <tr class="border-b border-neutral-100 last:border-0">
              <td class="px-4 py-3">
                <div class="font-medium">{dest.display_name}</div>
                {#if dest.subtitle}<div class="text-xs text-neutral-500">{dest.subtitle}</div>{/if}
              </td>
              <td class="px-4 py-3 text-neutral-600">{dest.config.folder_name ?? dest.config.folder_id}</td>
              <td class="px-4 py-3">
                {#if dest.accent_color}
                  <span class="inline-flex items-center gap-1.5">
                    <span class="h-3.5 w-3.5 rounded-full border border-neutral-300" style="background:{dest.accent_color}"></span>
                    <span class="text-xs text-neutral-500">{dest.accent_color}</span>
                  </span>
                {:else}
                  <span class="text-xs text-neutral-400">—</span>
                {/if}
              </td>
              <td class="px-4 py-3 text-right whitespace-nowrap">
                <a href="/destinations/{dest.id}" class="text-neutral-600 hover:text-neutral-900">Edit</a>
                <button
                  onclick={() => remove(dest.id, dest.display_name)}
                  disabled={deleting === dest.id}
                  class="ml-3 text-red-600 hover:text-red-700 disabled:opacity-50"
                >
                  {deleting === dest.id ? "Deleting…" : "Delete"}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
