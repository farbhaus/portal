<script lang="ts">
  import { invalidateAll } from "$app/navigation";
  import { Button, PathBreadcrumb, StatusPill, CopyButton } from "$lib/components";
  import type { UploadLink, DownloadLink } from "./+page.server";

  let { data } = $props();
  const uploadLinks = $derived(data.uploadLinks as UploadLink[]);
  const downloadLinks = $derived(data.downloadLinks as DownloadLink[]);

  let busy = $state<string | null>(null);

  function shortUrl(url: string): string {
    try { return new URL(url).pathname; } catch { return url; }
  }

  function sourceLabel(source: Record<string, unknown>): string {
    if (source.type === "file") return "1 file";
    if (source.type === "folder") return source.recursive ? "Folder (recursive)" : "Folder";
    if (source.type === "selection") return `${(source.file_ids as unknown[])?.length ?? 0} files`;
    return "—";
  }

  async function revokeUpload(id: string) {
    if (!confirm("Revoke this link? It will stop accepting uploads.")) return;
    busy = id;
    try {
      await fetch(`/api/upload-links/${id}/revoke`, { method: "POST" });
      await invalidateAll();
    } finally { busy = null; }
  }
  async function removeUpload(id: string) {
    if (!confirm("Delete this upload link permanently?")) return;
    busy = id;
    try {
      await fetch(`/api/upload-links/${id}`, { method: "DELETE" });
      await invalidateAll();
    } finally { busy = null; }
  }
  async function revokeDownload(id: string) {
    if (!confirm("Revoke this link? It will stop allowing downloads.")) return;
    busy = id;
    try {
      await fetch(`/api/download-links/${id}/revoke`, { method: "POST" });
      await invalidateAll();
    } finally { busy = null; }
  }
  async function removeDownload(id: string) {
    if (!confirm("Delete this download link permanently?")) return;
    busy = id;
    try {
      await fetch(`/api/download-links/${id}`, { method: "DELETE" });
      await invalidateAll();
    } finally { busy = null; }
  }
</script>

<div class="space-y-5">
  <div class="flex items-start justify-between gap-4">
    <div>
      <h1 class="text-xl font-semibold tracking-tight">Links</h1>
      <p class="mt-0.5 text-sm text-muted">Branded links to send files in or out — no Frame.io account needed.</p>
    </div>
    <Button href="/links/new">New link</Button>
  </div>

  <div class="grid gap-5 lg:grid-cols-2">
    <!-- Upload links -->
    <div class="rounded-card border border-border bg-surface shadow-sm">
      <div class="flex items-center justify-between border-b border-border px-5 py-3.5">
        <div>
          <h2 class="text-sm font-semibold">Upload links</h2>
          <p class="text-xs text-muted">Collaborators drop files into a destination.</p>
        </div>
        <span class="text-xs text-faint">{uploadLinks.length}</span>
      </div>

      {#if uploadLinks.length === 0}
        <div class="px-5 py-10 text-center">
          <p class="text-sm text-muted">No upload links yet.</p>
          <Button href="/links/new?type=upload" size="sm" class="mt-3">Create one</Button>
        </div>
      {:else}
        <div class="divide-y divide-border/60">
          {#each uploadLinks as link (link.id)}
            <div class="group px-5 py-3 hover:bg-surface-2 transition-colors">
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <a href="/upload-links/{link.id}" class="truncate font-medium text-sm hover:text-accent">{link.brand_display_name ?? "Untitled"}</a>
                    <StatusPill status={link.state} />
                  </div>
                  <div class="mt-0.5 text-xs text-muted">
                    {#if link.destination_path?.length}
                      <PathBreadcrumb segments={link.destination_path} class="text-xs" />
                    {:else}
                      {link.destination_name}
                    {/if}
                    {#if link.target_folder_name}<span class="text-faint">→ {link.target_folder_name}</span>{/if}
                  </div>
                  <CopyButton value={link.public_url} label={shortUrl(link.public_url)} class="mt-1 text-xs" />
                </div>
                <div class="hidden sm:flex shrink-0 items-center gap-2 opacity-0 group-hover:opacity-100 transition-opacity">
                  <a href={link.public_url} target="_blank" rel="noopener" class="text-xs text-muted hover:text-text">Open</a>
                  <a href="/upload-links/{link.id}" class="text-xs text-muted hover:text-text">Edit</a>
                  {#if link.state === "ok"}
                    <button onclick={() => revokeUpload(link.id)} disabled={busy === link.id} class="text-xs text-warning hover:opacity-80 disabled:opacity-50">Revoke</button>
                  {/if}
                  <button onclick={() => removeUpload(link.id)} disabled={busy === link.id} class="text-xs text-danger hover:opacity-80 disabled:opacity-50">Delete</button>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Download links -->
    <div class="rounded-card border border-border bg-surface shadow-sm">
      <div class="flex items-center justify-between border-b border-border px-5 py-3.5">
        <div>
          <h2 class="text-sm font-semibold">Download links</h2>
          <p class="text-xs text-muted">Send files out to recipients.</p>
        </div>
        <span class="text-xs text-faint">{downloadLinks.length}</span>
      </div>

      {#if downloadLinks.length === 0}
        <div class="px-5 py-10 text-center">
          <p class="text-sm text-muted">No download links yet.</p>
          <Button href="/links/new?type=download" size="sm" class="mt-3">Create one</Button>
        </div>
      {:else}
        <div class="divide-y divide-border/60">
          {#each downloadLinks as link (link.id)}
            <div class="group px-5 py-3 hover:bg-surface-2 transition-colors">
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <a href="/download-links/{link.id}" class="truncate font-medium text-sm hover:text-accent">{link.brand_display_name ?? "Untitled"}</a>
                    <StatusPill status={link.state} />
                  </div>
                  <div class="mt-0.5 text-xs text-muted">
                    {sourceLabel(link.source)}
                    {#if link.max_downloads}· {link.downloads_count}/{link.max_downloads} downloads{:else if link.downloads_count}· {link.downloads_count} downloads{/if}
                  </div>
                  <CopyButton value={link.public_url} label={shortUrl(link.public_url)} class="mt-1 text-xs" />
                </div>
                <div class="hidden sm:flex shrink-0 items-center gap-2 opacity-0 group-hover:opacity-100 transition-opacity">
                  <a href={link.public_url} target="_blank" rel="noopener" class="text-xs text-muted hover:text-text">Open</a>
                  <a href="/download-links/{link.id}" class="text-xs text-muted hover:text-text">Edit</a>
                  {#if link.state === "ok"}
                    <button onclick={() => revokeDownload(link.id)} disabled={busy === link.id} class="text-xs text-warning hover:opacity-80 disabled:opacity-50">Revoke</button>
                  {/if}
                  <button onclick={() => removeDownload(link.id)} disabled={busy === link.id} class="text-xs text-danger hover:opacity-80 disabled:opacity-50">Delete</button>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>
