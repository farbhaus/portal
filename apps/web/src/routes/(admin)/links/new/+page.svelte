<script lang="ts">
  import { goto, invalidateAll } from "$app/navigation";
  import { onMount } from "svelte";
  import {
    FolderPicker,
    FrameioSourcePicker,
    PathBreadcrumb,
    ProjectFolderBrowser,
    TemplateTokenInput,
    type DownloadSource,
    type FolderItem,
  } from "$lib/components";
  import { UPLOAD_TOKENS } from "$lib/tokens";
  import type { Destination } from "../../destinations/+page.server";

  let { data } = $props();

  // ── type selection ────────────────────────────────────────────────────────
  let linkType = $state<"upload" | "download" | null>(null);

  onMount(() => {
    const params = new URLSearchParams(window.location.search);
    const param = params.get("type");
    if (param === "upload" || param === "download") linkType = param;
    // "+ Upload link here" on a destination pre-selects it.
    const dest = params.get("destination");
    if (dest && data.destinations.some((d) => d.id === dest)) destinationId = dest;
  });

  // ── shared ────────────────────────────────────────────────────────────────
  let saving = $state(false);
  let error = $state<string | null>(null);

  // ── upload form state ─────────────────────────────────────────────────────
  const NEW_DEST = "__new__"; // select sentinel: create a destination inline, no page bounce
  let destinationId = $state(data.destinations[0]?.id ?? NEW_DEST);
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

  function destPath(dest: Destination): { id?: string; name: string }[] {
    if (dest.config.path?.length) return dest.config.path;
    return [{ name: dest.config.folder_name ?? dest.config.folder_id ?? "folder" }];
  }

  // ── inline destination creation ───────────────────────────────────────────
  let ndAccountId = $state("");
  let ndWorkspaceId = $state("");
  let ndProjectId = $state("");
  let ndProjectName = $state("");
  let ndFolderPath = $state<FolderItem[]>([]);
  let ndDisplayName = $state("");
  let creatingDest = $state(false);

  const ndCurrentFolder = $derived(ndFolderPath.at(-1) ?? null);

  async function createDestination() {
    if (!ndDisplayName.trim() || !ndCurrentFolder) return;
    creatingDest = true;
    error = null;
    try {
      const res = await fetch("/api/destinations", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          display_name: ndDisplayName.trim(),
          config: {
            type: "frameio",
            account_id: ndAccountId,
            workspace_id: ndWorkspaceId,
            project_id: ndProjectId,
            folder_id: ndCurrentFolder.id,
            // Sent so the backend caches the display breadcrumb without extra Frame.io calls.
            path: ndFolderPath,
            project_name: ndProjectName || null,
          },
        }),
      });
      if (!res.ok) {
        error = (await res.json().catch(() => ({}))).detail ?? `Could not create destination (${res.status})`;
        return;
      }
      const created = await res.json();
      await invalidateAll(); // refresh the destination list (with sync coverage) and select it
      destinationId = created.id;
      ndDisplayName = "";
      ndFolderPath = [];
    } finally {
      creatingDest = false;
    }
  }

  async function createUpload() {
    if (!destinationId || destinationId === NEW_DEST) { error = "Pick or create a destination first."; return; }
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

      <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
        <label class="block text-sm">
          <span class="text-muted">Destination <span class="text-danger">*</span></span>
          <select bind:value={destinationId} class="mt-1 w-full rounded-md border border-border px-2 py-1.5">
            {#each data.destinations as d (d.id)}<option value={d.id}>{d.display_name}</option>{/each}
            <option value={NEW_DEST}>＋ New destination…</option>
          </select>
        </label>

        {#if selectedDest}
          <!-- Where files land + whether that folder is synced, right at the point of decision. -->
          <div class="space-y-1.5 rounded-md border border-border bg-surface-2 p-3">
            <div class="text-xs">
              <span class="text-faint">Files land in:</span>
              <PathBreadcrumb segments={destPath(selectedDest)} class="text-xs" />
            </div>
            {#if selectedDest.sync_rules.length > 0}
              {#each selectedDest.sync_rules as sr (sr.id)}
                <div class="text-xs text-muted">
                  <span class={sr.enabled ? "text-success" : "text-faint"}>⟳</span>
                  {sr.enabled ? "Synced by" : "Sync rule (disabled):"}
                  <a href="/sync-rules/{sr.id}" class="font-medium hover:text-accent">{sr.name}</a>
                  {#if !sr.exact}<span class="text-faint">(via parent folder)</span>{/if}
                  <span class="text-faint">→</span>
                  <span class="font-mono">{sr.destination_path}</span>
                </div>
              {/each}
            {:else}
              <div class="text-xs text-muted">
                Not synced to local storage —
                <a href="/sync-rules/new?destination={selectedDest.id}" class="text-accent hover:underline">add a sync rule</a>
              </div>
            {/if}
          </div>
        {:else if destinationId === NEW_DEST}
          <div class="space-y-4 rounded-md border border-border bg-surface-2 p-3">
            <p class="text-xs text-muted">Pick the Frame.io folder files should land in and name it — it becomes a reusable destination.</p>
            <ProjectFolderBrowser
              bind:accountId={ndAccountId}
              bind:workspaceId={ndWorkspaceId}
              bind:projectId={ndProjectId}
              bind:projectName={ndProjectName}
              bind:folderPath={ndFolderPath}
            />
            <label class="block text-sm">
              <span class="text-muted">Destination name <span class="text-danger">*</span></span>
              <input bind:value={ndDisplayName} placeholder="e.g. DIT Drop — Project X" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
            </label>
            <button
              onclick={createDestination}
              disabled={creatingDest || !ndDisplayName.trim() || !ndCurrentFolder}
              class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-3 disabled:opacity-50"
            >
              {creatingDest ? "Creating…" : "Create destination"}
            </button>
          </div>
        {/if}
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
        <button onclick={createUpload} disabled={saving || destinationId === NEW_DEST} class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50">
          {saving ? "Creating…" : "Create upload link"}
        </button>
        <a href="/links" class="text-sm text-muted hover:text-text">Cancel</a>
      </div>
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
