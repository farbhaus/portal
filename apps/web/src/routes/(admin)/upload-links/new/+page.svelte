<script lang="ts">
  import { goto } from "$app/navigation";

  let { data } = $props();

  let destinationId = $state("");
  let expiresAt = $state("");
  let password = $state("");
  let maxSizeGb = $state("");
  let restrictExtensions = $state(false);
  let extensions = $state("");

  let seeded = false;
  $effect(() => {
    if (!seeded) {
      seeded = true;
      destinationId = data.destinations[0]?.id ?? "";
      extensions = data.extensionPreset.join(", ");
    }
  });
  let reqName = $state(false);
  let reqEmail = $state(false);
  let reqMessage = $state(false);
  let verifyEmail = $state(false);
  let brandDisplayName = $state("");
  let brandSubtitle = $state("");

  let saving = $state(false);
  let error = $state<string | null>(null);

  async function create() {
    if (!destinationId) {
      error = "Pick a destination first.";
      return;
    }
    saving = true;
    error = null;
    try {
      const body: Record<string, unknown> = {
        destination_id: destinationId,
        uploader_fields_required: { name: reqName, email: reqEmail, message: reqMessage },
        verify_email: verifyEmail,
      };
      if (expiresAt) body.expires_at = new Date(expiresAt).toISOString();
      if (password) body.password = password;
      if (maxSizeGb) body.max_file_size = Math.round(parseFloat(maxSizeGb) * 1024 ** 3);
      if (restrictExtensions) {
        body.allowed_extensions = extensions
          .split(",")
          .map((e) => e.trim())
          .filter(Boolean);
      }
      if (brandDisplayName.trim()) body.brand_display_name = brandDisplayName.trim();
      if (brandSubtitle.trim()) body.brand_subtitle = brandSubtitle.trim();

      const res = await fetch("/api/upload-links", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) {
        error = (await res.json().catch(() => ({}))).detail ?? `Could not create (${res.status})`;
        return;
      }
      await goto("/upload-links");
    } finally {
      saving = false;
    }
  }
</script>

<div class="max-w-2xl space-y-6">
  <div>
    <a href="/upload-links" class="text-sm text-muted hover:text-text">← Upload links</a>
    <h1 class="mt-1 text-2xl font-semibold">New upload link</h1>
  </div>

  {#if error}<p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>{/if}

  {#if data.destinations.length === 0}
    <div class="rounded-xl border border-warning/30 bg-warning/10 p-4 text-sm text-warning">
      You need a destination first. <a href="/destinations/new" class="font-medium underline">Create one</a>.
    </div>
  {:else}
    <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
      <label class="block text-sm">
        <span class="text-muted">Destination <span class="text-danger">*</span></span>
        <select bind:value={destinationId} class="mt-1 w-full rounded-md border border-border px-2 py-1.5">
          {#each data.destinations as d (d.id)}<option value={d.id}>{d.display_name}</option>{/each}
        </select>
      </label>

      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <label class="block text-sm">
          <span class="text-muted">Expires (optional)</span>
          <input type="datetime-local" bind:value={expiresAt} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
        </label>
        <label class="block text-sm">
          <span class="text-muted">Password (optional)</span>
          <input type="text" bind:value={password} placeholder="leave blank for none" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
        </label>
        <label class="block text-sm">
          <span class="text-muted">Max file size, GB (optional)</span>
          <input type="number" min="0" step="0.1" bind:value={maxSizeGb} placeholder="no limit" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
        </label>
      </div>

      <div class="text-sm">
        <label class="flex items-center gap-2">
          <input type="checkbox" bind:checked={restrictExtensions} />
          <span class="text-muted">Restrict file types</span>
        </label>
        {#if restrictExtensions}
          <textarea bind:value={extensions} rows="2" class="mt-2 w-full rounded-md border border-border px-2 py-1.5 text-xs"></textarea>
          <p class="mt-1 text-xs text-faint">Comma-separated extensions (prefilled with the media preset).</p>
        {/if}
      </div>

      <fieldset class="text-sm">
        <span class="text-muted">Required uploader fields</span>
        <div class="mt-1 flex gap-4">
          <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={reqName} /> Name</label>
          <label class="flex items-center gap-1.5"><input type="checkbox" checked={reqEmail || verifyEmail} disabled={verifyEmail} onchange={(e) => (reqEmail = e.currentTarget.checked)} /> Email</label>
          <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={reqMessage} /> Message</label>
        </div>
        <label class="mt-2 flex items-center gap-1.5">
          <input type="checkbox" bind:checked={verifyEmail} /> Verify email (sends a one-time code)
        </label>
      </fieldset>
    </div>

    <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
      <h2 class="font-medium">Branding overrides <span class="text-xs font-normal text-faint">(optional — falls back to the destination)</span></h2>
      <label class="block text-sm">
        <span class="text-muted">Display name</span>
        <input bind:value={brandDisplayName} placeholder="e.g. Send me your footage" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
      </label>
      <label class="block text-sm">
        <span class="text-muted">Subtitle</span>
        <input bind:value={brandSubtitle} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
      </label>
    </div>

    <div class="flex items-center gap-3">
      <button onclick={create} disabled={saving} class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50">
        {saving ? "Creating…" : "Create link"}
      </button>
      <a href="/upload-links" class="text-sm text-muted hover:text-text">Cancel</a>
    </div>
  {/if}
</div>
