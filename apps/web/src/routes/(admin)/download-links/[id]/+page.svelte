<script lang="ts">
  import { goto } from "$app/navigation";

  let { data } = $props();
  const link = $derived(data.link);

  let expiresAt = $state("");
  let newPassword = $state("");
  let clearPassword = $state(false);
  let maxDownloads = $state("");
  let reqName = $state(false);
  let reqEmail = $state(false);
  let verifyEmail = $state(false);
  let allowPreview = $state(true);
  let brandDisplayName = $state("");
  let brandSubtitle = $state("");

  let saving = $state(false);
  let error = $state<string | null>(null);
  let saved = $state(false);

  let seededId = "";
  $effect(() => {
    if (link.id !== seededId) {
      seededId = link.id;
      expiresAt = link.expires_at ? toLocalInput(link.expires_at) : "";
      maxDownloads = link.max_downloads ? String(link.max_downloads) : "";
      reqName = link.viewer_fields_required.name;
      reqEmail = link.viewer_fields_required.email;
      verifyEmail = link.verify_email;
      allowPreview = link.allow_preview;
      brandDisplayName = link.brand_display_name ?? "";
      brandSubtitle = link.brand_subtitle ?? "";
      newPassword = "";
      clearPassword = false;
    }
  });

  function toLocalInput(iso: string): string {
    const d = new Date(iso);
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  function sourceLabel(): string {
    const s = link.source as Record<string, unknown>;
    if (s.type === "file") return "Single file";
    if (s.type === "folder") return s.recursive ? "Folder (recursive)" : "Folder";
    if (s.type === "selection") return `${(s.file_ids as unknown[])?.length ?? 0} files`;
    return "—";
  }

  async function save() {
    saving = true;
    error = null;
    saved = false;
    try {
      const body: Record<string, unknown> = {
        expires_at: expiresAt ? new Date(expiresAt).toISOString() : null,
        max_downloads: maxDownloads ? parseInt(maxDownloads, 10) : null,
        viewer_fields_required: { name: reqName, email: reqEmail },
        verify_email: verifyEmail,
        allow_preview: allowPreview,
        brand_display_name: brandDisplayName.trim() || null,
        brand_subtitle: brandSubtitle.trim() || null,
      };
      if (clearPassword) body.password = "";
      else if (newPassword) body.password = newPassword;

      const res = await fetch(`/api/download-links/${link.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) {
        error = (await res.json().catch(() => ({}))).detail ?? `Could not save (${res.status})`;
        return;
      }
      saved = true;
      newPassword = "";
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!confirm("Delete this link permanently?")) return;
    await fetch(`/api/download-links/${link.id}`, { method: "DELETE" });
    await goto("/download-links");
  }
</script>

<div class="max-w-2xl space-y-6">
  <div>
    <a href="/download-links" class="text-sm text-neutral-500 hover:text-neutral-900">← Download links</a>
    <h1 class="mt-1 text-2xl font-semibold">Edit download link</h1>
  </div>

  {#if error}<p class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">{error}</p>
  {:else if saved}<p class="rounded-md bg-green-50 px-3 py-2 text-sm text-green-700">Saved.</p>{/if}

  <div class="rounded-xl border border-neutral-200 bg-white p-6 text-sm">
    <div class="flex items-center justify-between"><span class="text-neutral-500">Source</span><span>{sourceLabel()}</span></div>
    <div class="mt-2 flex items-center justify-between">
      <span class="text-neutral-500">Public URL</span>
      <a href={link.public_url} target="_blank" rel="noopener" class="max-w-xs truncate underline">{link.public_url}</a>
    </div>
    <div class="mt-2 flex items-center justify-between"><span class="text-neutral-500">Downloads</span><span>{link.downloads_count}{link.max_downloads ? ` / ${link.max_downloads}` : ""}</span></div>
    <div class="mt-2 flex items-center justify-between"><span class="text-neutral-500">Unique viewers</span><span>{data.stats.unique_viewers}</span></div>
    <div class="mt-2 flex items-center justify-between"><span class="text-neutral-500">Last activity</span><span>{data.stats.last_activity ? new Date(data.stats.last_activity).toLocaleString() : "—"}</span></div>
    <p class="mt-1 text-xs text-neutral-400">The source is fixed. Create a new link to share different files.</p>
  </div>

  <div class="space-y-4 rounded-xl border border-neutral-200 bg-white p-6">
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <label class="block text-sm">
        <span class="text-neutral-500">Expires</span>
        <input type="datetime-local" bind:value={expiresAt} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
      </label>
      <label class="block text-sm">
        <span class="text-neutral-500">Max downloads</span>
        <input type="number" min="1" bind:value={maxDownloads} placeholder="unlimited" class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
      </label>
    </div>
    <div class="text-sm">
      <span class="text-neutral-500">Password</span>
      {#if link.password_protected}<span class="ml-2 text-xs text-green-700">currently set</span>{/if}
      <input type="text" bind:value={newPassword} disabled={clearPassword} placeholder={link.password_protected ? "enter to change" : "set a password"} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5 disabled:bg-neutral-50" />
      {#if link.password_protected}
        <label class="mt-1 flex items-center gap-1.5 text-xs text-neutral-500"><input type="checkbox" bind:checked={clearPassword} /> Remove password</label>
      {/if}
    </div>
    <div class="flex flex-wrap gap-4 text-sm">
      <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={reqName} /> Require name</label>
      <label class="flex items-center gap-1.5"><input type="checkbox" checked={reqEmail || verifyEmail} disabled={verifyEmail} onchange={(e) => (reqEmail = e.currentTarget.checked)} /> Require email</label>
      <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={verifyEmail} /> Verify email (one-time code)</label>
      <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={allowPreview} /> Show thumbnails/previews</label>
    </div>
  </div>

  <div class="space-y-4 rounded-xl border border-neutral-200 bg-white p-6">
    <h2 class="font-medium">Branding</h2>
    <label class="block text-sm">
      <span class="text-neutral-500">Display name</span>
      <input bind:value={brandDisplayName} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-neutral-500">Subtitle</span>
      <input bind:value={brandSubtitle} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
    </label>
  </div>

  <div class="flex items-center justify-between">
    <div class="flex items-center gap-3">
      <button onclick={save} disabled={saving} class="rounded-md bg-neutral-900 px-4 py-2 text-sm font-medium text-white hover:bg-neutral-800 disabled:opacity-50">
        {saving ? "Saving…" : "Save changes"}
      </button>
      <a href="/download-links" class="text-sm text-neutral-500 hover:text-neutral-900">Cancel</a>
    </div>
    <button onclick={remove} class="text-sm text-red-600 hover:text-red-700">Delete</button>
  </div>

  <div class="space-y-3 rounded-xl border border-neutral-200 bg-white p-6">
    <h2 class="font-medium">Download log <span class="text-xs font-normal text-neutral-400">({data.stats.downloads})</span></h2>
    {#if data.events.length === 0}
      <p class="text-sm text-neutral-400">No downloads yet.</p>
    {:else}
      <div class="overflow-hidden rounded-lg border border-neutral-100">
        <table class="w-full text-sm">
          <thead class="border-b border-neutral-100 text-left text-neutral-500">
            <tr>
              <th class="px-3 py-2 font-medium">When</th>
              <th class="px-3 py-2 font-medium">Viewer</th>
              <th class="px-3 py-2 font-medium">File</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-neutral-50">
            {#each data.events as ev (ev.started_at + ev.frameio_file_id)}
              <tr>
                <td class="px-3 py-2 text-xs whitespace-nowrap text-neutral-400">{new Date(ev.started_at).toLocaleString()}</td>
                <td class="px-3 py-2 text-neutral-600">{ev.viewer_email ?? ev.viewer_name ?? ev.ip ?? "—"}</td>
                <td class="px-3 py-2 font-mono text-xs">{ev.file_name ?? ev.frameio_file_id}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</div>
