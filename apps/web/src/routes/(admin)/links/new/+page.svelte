<script lang="ts">
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import { FolderPicker, FrameioSourcePicker, TemplateTokenInput, type DownloadSource } from "$lib/components";
  import { UPLOAD_TOKENS } from "$lib/tokens";

  let { data } = $props();

  // ── type selection ────────────────────────────────────────────────────────
  let linkType = $state<"upload" | "download" | null>(null);

  onMount(() => {
    const param = new URLSearchParams(window.location.search).get("type");
    if (param === "upload" || param === "download") linkType = param;
  });

  // ── shared ────────────────────────────────────────────────────────────────
  let saving = $state(false);
  let error = $state<string | null>(null);

  // ── upload form state ─────────────────────────────────────────────────────
  let destinationId = $state(data.destinations[0]?.id ?? "");
  let expiresAt = $state("");
  let password = $state("");
  let maxSizeGb = $state("");
  let restrictExtensions = $state(false);
  let extensions = $state(data.extensionPreset.join(", "));
  let reqName = $state(false);
  let reqEmail = $state(false);
  let reqMessage = $state(false);
  let verifyEmail = $state(false);
  let targetFolder = $state<{ id: string; name: string } | null>(null);
  let subfolderTemplate = $state("");
  let uBrandDisplayName = $state("");
  let uBrandSubtitle = $state("");

  const selectedDest = $derived(data.destinations.find((d) => d.id === destinationId));

  async function createUpload() {
    if (!destinationId) { error = "Pick a destination first."; return; }
    saving = true; error = null;
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
        body.allowed_extensions = extensions.split(",").map((e) => e.trim()).filter(Boolean);
      }
      // Only record a target subfolder when it differs from the destination root.
      if (targetFolder && targetFolder.id !== selectedDest?.config.folder_id) {
        body.target_folder_id = targetFolder.id;
        body.target_folder_name = targetFolder.name;
      }
      if (subfolderTemplate.trim()) body.subfolder_template = subfolderTemplate.trim();
      if (uBrandDisplayName.trim()) body.brand_display_name = uBrandDisplayName.trim();
      if (uBrandSubtitle.trim()) body.brand_subtitle = uBrandSubtitle.trim();
      const res = await fetch("/api/upload-links", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) { error = (await res.json().catch(() => ({}))).detail ?? `Could not create (${res.status})`; return; }
      await goto("/links");
    } finally { saving = false; }
  }

  // ── download form state ───────────────────────────────────────────────────
  let downloadSource = $state<DownloadSource | null>(null);
  let dExpiresAt = $state("");
  let dPassword = $state("");
  let maxDownloads = $state("");
  let dReqName = $state(false);
  let dReqEmail = $state(false);
  let dVerifyEmail = $state(false);
  let allowPreview = $state(true);
  let dBrandDisplayName = $state("");
  let dBrandSubtitle = $state("");

  async function createDownload() {
    const source = downloadSource;
    if (!source) { error = "Pick a file, some files, or a folder to share."; return; }
    saving = true; error = null;
    try {
      const body: Record<string, unknown> = {
        source,
        viewer_fields_required: { name: dReqName, email: dReqEmail },
        verify_email: dVerifyEmail,
        allow_preview: allowPreview,
      };
      if (dExpiresAt) body.expires_at = new Date(dExpiresAt).toISOString();
      if (dPassword) body.password = dPassword;
      if (maxDownloads) body.max_downloads = parseInt(maxDownloads, 10);
      if (dBrandDisplayName.trim()) body.brand_display_name = dBrandDisplayName.trim();
      if (dBrandSubtitle.trim()) body.brand_subtitle = dBrandSubtitle.trim();
      const res = await fetch("/api/download-links", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) { error = (await res.json().catch(() => ({}))).detail ?? `Could not create (${res.status})`; return; }
      await goto("/links");
    } finally { saving = false; }
  }
</script>

<div class="mx-auto max-w-2xl space-y-6">
  <div>
    <a href="/links" class="text-sm text-muted hover:text-text">← Links</a>
    <h1 class="mt-1 text-2xl font-semibold">New link</h1>
  </div>

  {#if error}<p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>{/if}

  <!-- Type picker -->
  {#if !linkType}
    <div class="grid grid-cols-2 gap-4">
      <button
        onclick={() => (linkType = "upload")}
        class="group rounded-xl border border-border bg-surface p-6 text-left hover:border-accent/50 hover:bg-surface-2 transition-colors"
      >
        <svg class="mb-3 h-8 w-8 text-accent" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M17 8l-5-5-5 5M12 3v12" />
        </svg>
        <div class="font-semibold">Upload link</div>
        <div class="mt-1 text-sm text-muted">Collaborators drop files into a destination.</div>
      </button>
      <button
        onclick={() => (linkType = "download")}
        class="group rounded-xl border border-border bg-surface p-6 text-left hover:border-accent/50 hover:bg-surface-2 transition-colors"
      >
        <svg class="mb-3 h-8 w-8 text-accent" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3" />
        </svg>
        <div class="font-semibold">Download link</div>
        <div class="mt-1 text-sm text-muted">Send files out to recipients from Frame.io.</div>
      </button>
    </div>
  {/if}

  <!-- Upload form -->
  {#if linkType === "upload"}
    <div class="flex items-center gap-2">
      <span class="text-sm font-medium">Upload link</span>
      <button onclick={() => (linkType = null)} class="text-xs text-faint hover:text-muted">change type</button>
    </div>

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
            <input type="checkbox" bind:checked={verifyEmail} /> Require OTP (one-time passcode by email)
          </label>
        </fieldset>
      </div>

      {#if selectedDest?.config.account_id && selectedDest?.config.folder_id}
        <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
          <h2 class="font-medium">Where uploads land <span class="text-xs font-normal text-faint">(optional)</span></h2>
          <div>
            <p class="mb-2 text-sm text-muted">Pick a base subfolder under the destination (defaults to its root).</p>
            {#key destinationId}
              <FolderPicker
                accountId={selectedDest.config.account_id}
                rootFolderId={selectedDest.config.folder_id}
                rootName={selectedDest.display_name}
                bind:value={targetFolder}
              />
            {/key}
          </div>
          <div class="block text-sm">
            <span class="text-muted">Path template (optional)</span>
            <TemplateTokenInput bind:value={subfolderTemplate} tokens={UPLOAD_TOKENS} placeholder={"e.g. {date}/{uploader_name}"} />
            <p class="mt-1 text-xs text-faint">Folders created per upload beneath the base.</p>
          </div>
        </div>
      {/if}

      <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
        <h2 class="font-medium">Branding <span class="text-xs font-normal text-faint">(optional — falls back to the destination)</span></h2>
        <label class="block text-sm">
          <span class="text-muted">Display name</span>
          <input bind:value={uBrandDisplayName} placeholder="e.g. Send me your footage" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
        </label>
        <label class="block text-sm">
          <span class="text-muted">Subtitle</span>
          <input bind:value={uBrandSubtitle} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
        </label>
      </div>

      <div class="flex items-center gap-3">
        <button onclick={createUpload} disabled={saving} class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50">
          {saving ? "Creating…" : "Create upload link"}
        </button>
        <a href="/links" class="text-sm text-muted hover:text-text">Cancel</a>
      </div>
    {/if}
  {/if}

  <!-- Download form -->
  {#if linkType === "download"}
    <div class="flex items-center gap-2">
      <span class="text-sm font-medium">Download link</span>
      <button onclick={() => (linkType = null)} class="text-xs text-faint hover:text-muted">change type</button>
    </div>

    <div class="space-y-5 rounded-xl border border-border bg-surface p-6">
      <h2 class="font-medium">Source</h2>
      <FrameioSourcePicker bind:value={downloadSource} onerror={(m) => (error = m)} />
    </div>

    <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
      <h2 class="font-medium">Options</h2>
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <label class="block text-sm">
          <span class="text-muted">Expires</span>
          <input type="datetime-local" bind:value={dExpiresAt} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
        </label>
        <label class="block text-sm">
          <span class="text-muted">Password</span>
          <input type="text" bind:value={dPassword} placeholder="none" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
        </label>
        <label class="block text-sm">
          <span class="text-muted">Max downloads</span>
          <input type="number" min="1" bind:value={maxDownloads} placeholder="unlimited" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
        </label>
      </div>
      <div class="flex flex-wrap gap-4 text-sm">
        <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={dReqName} /> Require name</label>
        <label class="flex items-center gap-1.5"><input type="checkbox" checked={dReqEmail || dVerifyEmail} disabled={dVerifyEmail} onchange={(e) => (dReqEmail = e.currentTarget.checked)} /> Require email</label>
        <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={dVerifyEmail} /> Require OTP</label>
        <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={allowPreview} /> Show thumbnails</label>
      </div>
    </div>

    <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
      <h2 class="font-medium">Branding</h2>
      <label class="block text-sm">
        <span class="text-muted">Display name</span>
        <input bind:value={dBrandDisplayName} placeholder="e.g. Your deliverables" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
      </label>
      <label class="block text-sm">
        <span class="text-muted">Subtitle</span>
        <input bind:value={dBrandSubtitle} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
      </label>
    </div>

    <div class="flex items-center gap-3">
      <button onclick={createDownload} disabled={saving || !downloadSource} class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50">
        {saving ? "Creating…" : "Create download link"}
      </button>
      <a href="/links" class="text-sm text-muted hover:text-text">Cancel</a>
    </div>
  {/if}
</div>
