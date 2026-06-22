<script lang="ts">
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import { FolderPicker, ProjectPicker, TemplateTokenInput } from "$lib/components";
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
  type Item = { id: string; name: string };
  type Project = { id: string; name: string; root_folder_id: string | null };
  type FileItem = { id: string; name: string; file_size: number | null };

  let subfolders = $state<Item[]>([]);
  let files = $state<FileItem[]>([]);
  let accountId = $state("");
  let workspaceId = $state("");
  let projectId = $state("");
  let folderPath = $state<Item[]>([]);
  let selected = $state<Map<string, string>>(new Map());
  let folderShare = $state<Item | null>(null);
  let recursive = $state(false);
  let dExpiresAt = $state("");
  let dPassword = $state("");
  let maxDownloads = $state("");
  let dReqName = $state(false);
  let dReqEmail = $state(false);
  let dVerifyEmail = $state(false);
  let allowPreview = $state(true);
  let dBrandDisplayName = $state("");
  let dBrandSubtitle = $state("");
  let loadingFrameio = $state(false);
  let newFolderName = $state("");
  let creatingFolder = $state(false);

  const currentFolder = $derived(folderPath.at(-1) ?? null);
  const hasSource = $derived(folderShare !== null || selected.size > 0);

  async function getJSON<T>(url: string): Promise<T> {
    const res = await fetch(url);
    if (!res.ok) throw new Error((await res.json().catch(() => ({}))).detail ?? `Failed (${res.status})`);
    return res.json();
  }
  async function loadFrameio<T>(fn: () => Promise<T>) {
    loadingFrameio = true; error = null;
    try { return await fn(); }
    catch (e) { error = e instanceof Error ? e.message : "Failed to load from Frame.io"; }
    finally { loadingFrameio = false; }
  }

  function onScopeReset() {
    projectId = ""; subfolders = files = []; folderPath = []; clearSource();
  }
  async function onProjectSelect(proj: Project) {
    projectId = proj.id;
    subfolders = files = []; folderPath = []; clearSource();
    if (!proj.root_folder_id) return;
    folderPath = [{ id: proj.root_folder_id, name: `${proj.name} (root)` }];
    await loadFolder();
  }
  async function loadFolder() {
    if (!currentFolder) return;
    await loadFrameio(async () => {
      subfolders = await getJSON<Item[]>(`/api/frameio/folders?account_id=${accountId}&folder_id=${currentFolder.id}`);
      files = await getJSON<FileItem[]>(`/api/frameio/files?account_id=${accountId}&folder_id=${currentFolder.id}`);
    });
  }
  async function drillInto(folder: Item) { folderPath = [...folderPath, folder]; await loadFolder(); }
  async function jumpTo(index: number) { folderPath = folderPath.slice(0, index + 1); await loadFolder(); }

  async function createFolder() {
    const name = newFolderName.trim();
    if (!name || !currentFolder) return;
    creatingFolder = true; error = null;
    try {
      const res = await fetch("/api/frameio/folders", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ account_id: accountId, parent_folder_id: currentFolder.id, name }),
      });
      if (!res.ok) { error = (await res.json().catch(() => ({}))).detail ?? `Could not create folder (${res.status})`; return; }
      const folder = (await res.json()) as Item;
      newFolderName = "";
      await drillInto(folder);
    } finally { creatingFolder = false; }
  }

  function toggleFile(f: FileItem) {
    folderShare = null;
    const next = new Map(selected);
    if (next.has(f.id)) next.delete(f.id); else next.set(f.id, f.name);
    selected = next;
  }
  function shareFolder() { if (!currentFolder) return; selected = new Map(); folderShare = currentFolder; }
  function clearSource() { selected = new Map(); folderShare = null; recursive = false; }

  function buildSource(): Record<string, unknown> | null {
    if (folderShare) return { type: "folder", account_id: accountId, folder_id: folderShare.id, recursive };
    const ids = [...selected.keys()];
    if (ids.length === 1) return { type: "file", account_id: accountId, file_id: ids[0] };
    if (ids.length > 1) return { type: "selection", account_id: accountId, file_ids: ids };
    return null;
  }

  async function createDownload() {
    const source = buildSource();
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
      <ProjectPicker
        bind:accountId
        bind:workspaceId
        selectedProjectId={projectId}
        onselect={onProjectSelect}
        onscopechange={onScopeReset}
      />

      {#if folderPath.length > 0}
        <div class="rounded-md border border-border bg-surface-2 p-3">
          <div class="flex flex-wrap items-center gap-1 text-sm">
            {#each folderPath as f, i (f.id)}
              {#if i > 0}<span class="text-faint">/</span>{/if}
              <button onclick={() => jumpTo(i)} class="rounded px-1 hover:bg-surface-3 {i === folderPath.length - 1 ? 'font-medium' : 'text-muted'}">{f.name}</button>
            {/each}
            <button onclick={shareFolder} class="ml-auto rounded border border-border bg-surface px-2 py-0.5 text-xs hover:bg-surface-2">Share this whole folder</button>
          </div>
          <div class="mt-3 space-y-1">
            {#if loadingFrameio}<p class="text-xs text-faint">Loading…</p>{/if}
            {#each subfolders as sf (sf.id)}
              <button onclick={() => drillInto(sf)} class="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm hover:bg-surface-3">
                <span class="text-faint">📁</span> {sf.name}
              </button>
            {/each}
            {#each files as f (f.id)}
              <label class="flex w-full cursor-pointer items-center gap-2 rounded px-2 py-1 text-sm hover:bg-surface-3">
                <input type="checkbox" checked={selected.has(f.id)} onchange={() => toggleFile(f)} />
                <span class="text-faint">📄</span> {f.name}
              </label>
            {/each}
            {#if !loadingFrameio && subfolders.length === 0 && files.length === 0}
              <p class="text-xs text-faint">This folder is empty.</p>
            {/if}
          </div>
          <div class="mt-3 flex items-center gap-2">
            <input bind:value={newFolderName} placeholder="New subfolder name"
              onkeydown={(e) => e.key === "Enter" && createFolder()}
              class="flex-1 rounded-md border border-border bg-surface px-2 py-1 text-sm" />
            <button onclick={createFolder} disabled={!newFolderName.trim() || creatingFolder}
              class="shrink-0 rounded-md border border-border px-2.5 py-1 text-xs hover:bg-surface-3 disabled:opacity-50">
              {creatingFolder ? "Creating…" : "＋ New folder"}
            </button>
          </div>
        </div>
      {/if}

      {#if hasSource}
        <div class="rounded-md bg-accent px-3 py-2 text-sm text-on-accent">
          {#if folderShare}
            <div class="flex items-center justify-between">
              <span>Sharing folder: <span class="font-medium">{folderShare.name}</span></span>
              <button onclick={clearSource} class="text-xs text-faint hover:text-on-accent">clear</button>
            </div>
            <label class="mt-1 flex items-center gap-1.5 text-xs text-faint">
              <input type="checkbox" bind:checked={recursive} /> Include subfolders (recursive)
            </label>
          {:else}
            <div class="flex items-center justify-between">
              <span>{selected.size} file{selected.size === 1 ? "" : "s"} selected</span>
              <button onclick={clearSource} class="text-xs text-faint hover:text-on-accent">clear</button>
            </div>
          {/if}
        </div>
      {/if}
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
      <button onclick={createDownload} disabled={saving || !hasSource} class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50">
        {saving ? "Creating…" : "Create download link"}
      </button>
      <a href="/links" class="text-sm text-muted hover:text-text">Cancel</a>
    </div>
  {/if}
</div>
