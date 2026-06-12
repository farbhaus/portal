<script lang="ts">
  import { invalidateAll } from "$app/navigation";
  import { PageHeader, Tabs, Button, Table, StatusPill, CopyButton, EmptyState } from "$lib/components";

  let { data } = $props();
  let busy = $state<string | null>(null);

  const linkTabs = [
    { label: "Upload", href: "/upload-links" },
    { label: "Download", href: "/download-links" },
  ];

  async function revoke(id: string) {
    if (!confirm("Revoke this link? It will stop accepting uploads.")) return;
    busy = id;
    try {
      await fetch(`/api/upload-links/${id}/revoke`, { method: "POST" });
      await invalidateAll();
    } finally {
      busy = null;
    }
  }

  async function remove(id: string) {
    if (!confirm("Delete this link permanently?")) return;
    busy = id;
    try {
      await fetch(`/api/upload-links/${id}`, { method: "DELETE" });
      await invalidateAll();
    } finally {
      busy = null;
    }
  }
</script>

<div class="space-y-5">
  <PageHeader title="Links" subtitle="Branded links for collaborators to drop files into a destination.">
    {#snippet actions()}
      <Button href="/upload-links/new">New upload link</Button>
    {/snippet}
  </PageHeader>

  <Tabs tabs={linkTabs} />

  {#if data.links.length === 0}
    <EmptyState message="No upload links yet.">
      <Button href="/upload-links/new" size="sm">Create one</Button>
    </EmptyState>
  {:else}
    <Table columns={["Link", "Destination", "Status", "Protections", { label: "", class: "text-right" }]}>
      {#each data.links as link (link.id)}
        <tr class="align-top">
          <td class="px-4 py-3">
            <div class="font-medium">{link.brand_display_name ?? "Untitled"}</div>
            <CopyButton value={link.public_url} class="mt-0.5 max-w-xs text-xs" />
          </td>
          <td class="px-4 py-3 text-muted">{link.destination_name}</td>
          <td class="px-4 py-3"><StatusPill status={link.state} /></td>
          <td class="px-4 py-3 text-xs text-muted">
            {link.password_protected ? "🔒 password " : ""}
            {link.verify_email ? "✉︎ OTP " : ""}
            {link.expires_at ? `⏰ ${new Date(link.expires_at).toLocaleDateString()} ` : ""}
            {link.allowed_extensions ? `· ${link.allowed_extensions.length} types` : ""}
            {#if !link.password_protected && !link.verify_email && !link.expires_at && !link.allowed_extensions}—{/if}
          </td>
          <td class="px-4 py-3 text-right whitespace-nowrap">
            <a href={link.public_url} target="_blank" rel="noopener" class="text-muted hover:text-text">Open</a>
            <a href="/upload-links/{link.id}" class="ml-3 text-muted hover:text-text">Edit</a>
            {#if link.state === "ok"}
              <button onclick={() => revoke(link.id)} disabled={busy === link.id} class="ml-3 text-warning hover:opacity-80 disabled:opacity-50">Revoke</button>
            {/if}
            <button onclick={() => remove(link.id)} disabled={busy === link.id} class="ml-3 text-danger hover:opacity-80 disabled:opacity-50">Delete</button>
          </td>
        </tr>
      {/each}
    </Table>
  {/if}
</div>
