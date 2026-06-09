<script lang="ts">
  import { invalidateAll } from "$app/navigation";

  let { data } = $props();
  let busy = $state<string | null>(null);
  let copied = $state<string | null>(null);

  async function copy(url: string, id: string) {
    await navigator.clipboard.writeText(url);
    copied = id;
    setTimeout(() => (copied = copied === id ? null : copied), 1500);
  }

  async function revoke(id: string) {
    if (!confirm("Revoke this link? It will stop allowing downloads.")) return;
    busy = id;
    try {
      await fetch(`/api/download-links/${id}/revoke`, { method: "POST" });
      await invalidateAll();
    } finally {
      busy = null;
    }
  }

  async function remove(id: string) {
    if (!confirm("Delete this link permanently?")) return;
    busy = id;
    try {
      await fetch(`/api/download-links/${id}`, { method: "DELETE" });
      await invalidateAll();
    } finally {
      busy = null;
    }
  }

  function sourceLabel(source: Record<string, unknown>): string {
    if (source.type === "file") return "1 file";
    if (source.type === "folder") return source.recursive ? "Folder (recursive)" : "Folder";
    if (source.type === "selection") return `${(source.file_ids as unknown[])?.length ?? 0} files`;
    return "—";
  }

  const stateClass: Record<string, string> = {
    ok: "bg-green-100 text-green-800",
    expired: "bg-amber-100 text-amber-800",
    revoked: "bg-neutral-200 text-neutral-600",
  };
</script>

<div class="max-w-5xl space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold">Download links</h1>
      <p class="text-sm text-neutral-500">Branded links to send files out to recipients (no Frame.io account needed).</p>
    </div>
    <a href="/download-links/new" class="rounded-md bg-neutral-900 px-4 py-2 text-sm font-medium text-white hover:bg-neutral-800">
      New link
    </a>
  </div>

  {#if data.links.length === 0}
    <div class="rounded-xl border border-dashed border-neutral-300 bg-white p-10 text-center">
      <p class="text-sm text-neutral-500">No download links yet.</p>
      <a href="/download-links/new" class="mt-2 inline-block text-sm font-medium underline">Create one</a>
    </div>
  {:else}
    <div class="overflow-hidden rounded-xl border border-neutral-200 bg-white">
      <table class="w-full text-sm">
        <thead class="border-b border-neutral-200 text-left text-neutral-500">
          <tr>
            <th class="px-4 py-2.5 font-medium">Link</th>
            <th class="px-4 py-2.5 font-medium">Source</th>
            <th class="px-4 py-2.5 font-medium">Status</th>
            <th class="px-4 py-2.5 font-medium">Downloads</th>
            <th class="px-4 py-2.5"></th>
          </tr>
        </thead>
        <tbody>
          {#each data.links as link (link.id)}
            <tr class="border-b border-neutral-100 align-top last:border-0">
              <td class="px-4 py-3">
                <div class="font-medium">{link.brand_display_name ?? "—"}</div>
                <button onclick={() => copy(link.public_url, link.id)} class="mt-0.5 max-w-xs truncate text-xs text-neutral-500 hover:text-neutral-900" title={link.public_url}>
                  {copied === link.id ? "Copied!" : link.public_url}
                </button>
              </td>
              <td class="px-4 py-3 text-neutral-600">{sourceLabel(link.source)}</td>
              <td class="px-4 py-3">
                <span class="rounded-full px-2 py-0.5 text-xs font-medium {stateClass[link.state]}">{link.state}</span>
              </td>
              <td class="px-4 py-3 text-neutral-600">
                {link.downloads_count}{link.max_downloads ? ` / ${link.max_downloads}` : ""}
              </td>
              <td class="px-4 py-3 text-right whitespace-nowrap">
                <a href={link.public_url} target="_blank" rel="noopener" class="text-neutral-600 hover:text-neutral-900">Open</a>
                <a href="/download-links/{link.id}" class="ml-3 text-neutral-600 hover:text-neutral-900">Edit</a>
                {#if link.state === "ok"}
                  <button onclick={() => revoke(link.id)} disabled={busy === link.id} class="ml-3 text-amber-700 hover:text-amber-800 disabled:opacity-50">Revoke</button>
                {/if}
                <button onclick={() => remove(link.id)} disabled={busy === link.id} class="ml-3 text-red-600 hover:text-red-700 disabled:opacity-50">Delete</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
