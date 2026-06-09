<script lang="ts">
  import { goto } from "$app/navigation";

  let { data } = $props();
  const link = $derived(data.link);

  let expiresAt = $state("");
  let newPassword = $state("");
  let clearPassword = $state(false);
  let maxSizeGb = $state("");
  let restrictExtensions = $state(false);
  let extensions = $state("");
  let reqName = $state(false);
  let reqEmail = $state(false);
  let reqMessage = $state(false);
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
      maxSizeGb = link.max_file_size ? (link.max_file_size / 1024 ** 3).toString() : "";
      restrictExtensions = !!link.allowed_extensions;
      extensions = (link.allowed_extensions ?? []).join(", ");
      reqName = link.uploader_fields_required.name;
      reqEmail = link.uploader_fields_required.email;
      reqMessage = link.uploader_fields_required.message;
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

  async function save() {
    saving = true;
    error = null;
    saved = false;
    try {
      const body: Record<string, unknown> = {
        expires_at: expiresAt ? new Date(expiresAt).toISOString() : null,
        max_file_size: maxSizeGb ? Math.round(parseFloat(maxSizeGb) * 1024 ** 3) : null,
        allowed_extensions: restrictExtensions
          ? extensions.split(",").map((e) => e.trim()).filter(Boolean)
          : null,
        uploader_fields_required: { name: reqName, email: reqEmail, message: reqMessage },
        brand_display_name: brandDisplayName.trim() || null,
        brand_subtitle: brandSubtitle.trim() || null,
      };
      if (clearPassword) body.password = "";
      else if (newPassword) body.password = newPassword;

      const res = await fetch(`/api/upload-links/${link.id}`, {
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

  async function revoke() {
    if (!confirm("Revoke this link?")) return;
    await fetch(`/api/upload-links/${link.id}/revoke`, { method: "POST" });
    await goto("/upload-links");
  }

  async function remove() {
    if (!confirm("Delete this link permanently?")) return;
    await fetch(`/api/upload-links/${link.id}`, { method: "DELETE" });
    await goto("/upload-links");
  }
</script>

<div class="max-w-2xl space-y-6">
  <div>
    <a href="/upload-links" class="text-sm text-neutral-500 hover:text-neutral-900">← Upload links</a>
    <h1 class="mt-1 text-2xl font-semibold">Edit upload link</h1>
  </div>

  {#if error}<p class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">{error}</p>
  {:else if saved}<p class="rounded-md bg-green-50 px-3 py-2 text-sm text-green-700">Saved.</p>{/if}

  <div class="rounded-xl border border-neutral-200 bg-white p-6 text-sm">
    <div class="flex items-center justify-between">
      <span class="text-neutral-500">Destination</span><span>{link.destination_name}</span>
    </div>
    <div class="mt-2 flex items-center justify-between">
      <span class="text-neutral-500">Public URL</span>
      <a href={link.public_url} target="_blank" rel="noopener" class="max-w-xs truncate underline">{link.public_url}</a>
    </div>
    <div class="mt-2 flex items-center justify-between">
      <span class="text-neutral-500">Status</span><span>{link.state}</span>
    </div>
  </div>

  <div class="space-y-4 rounded-xl border border-neutral-200 bg-white p-6">
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <label class="block text-sm">
        <span class="text-neutral-500">Expires</span>
        <input type="datetime-local" bind:value={expiresAt} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
      </label>
      <label class="block text-sm">
        <span class="text-neutral-500">Max file size, GB</span>
        <input type="number" min="0" step="0.1" bind:value={maxSizeGb} placeholder="no limit" class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
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

    <div class="text-sm">
      <label class="flex items-center gap-2"><input type="checkbox" bind:checked={restrictExtensions} /> <span class="text-neutral-600">Restrict file types</span></label>
      {#if restrictExtensions}
        <textarea bind:value={extensions} rows="2" class="mt-2 w-full rounded-md border border-neutral-300 px-2 py-1.5 text-xs"></textarea>
      {/if}
    </div>

    <fieldset class="text-sm">
      <span class="text-neutral-500">Required uploader fields</span>
      <div class="mt-1 flex gap-4">
        <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={reqName} /> Name</label>
        <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={reqEmail} /> Email</label>
        <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={reqMessage} /> Message</label>
      </div>
    </fieldset>
  </div>

  <div class="space-y-4 rounded-xl border border-neutral-200 bg-white p-6">
    <h2 class="font-medium">Branding overrides</h2>
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
      <a href="/upload-links" class="text-sm text-neutral-500 hover:text-neutral-900">Cancel</a>
    </div>
    <div class="flex items-center gap-3">
      {#if link.state === "ok"}<button onclick={revoke} class="text-sm text-amber-700 hover:text-amber-800">Revoke</button>{/if}
      <button onclick={remove} class="text-sm text-red-600 hover:text-red-700">Delete</button>
    </div>
  </div>
</div>
