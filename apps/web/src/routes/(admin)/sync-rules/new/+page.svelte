<script lang="ts">
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import {
    ProjectFolderBrowser,
    TemplateTokenInput,
    type FolderBrowserInit,
    type FolderItem,
  } from "$lib/components";
  import { SYNC_TOKENS } from "$lib/tokens";

  let accountId = $state("");
  let workspaceId = $state("");
  let projectId = $state("");
  let projectName = $state("");
  let folderPath = $state<FolderItem[]>([]);
  let chosenFolder = $state<FolderItem | null>(null);

  // Options
  let name = $state("");
  let destinationPath = $state("");
  let recursive = $state(true);
  let conflictPolicy = $state("rename_suffix");
  let pathTemplate = $state("");
  let enabled = $state(true);

  let error = $state<string | null>(null);
  let saving = $state(false);

  const currentFolder = $derived(folderPath.at(-1) ?? null);

  // "+ Sync rule here" on a destination: seed the browser at the destination's folder. The
  // browser only renders once the prefill is resolved, so the pickers mount pre-seeded.
  let prefill = $state<FolderBrowserInit | undefined>(undefined);
  let prefillLoading = $state(true);

  onMount(async () => {
    const destId = new URLSearchParams(window.location.search).get("destination");
    if (!destId) {
      prefillLoading = false;
      return;
    }
    try {
      const res = await fetch(`/api/destinations/${destId}`);
      if (res.ok) {
        const dest = await res.json();
        const cfg = dest.config ?? {};
        if (cfg.account_id && cfg.workspace_id && cfg.project_id && cfg.folder_id) {
          const path: FolderItem[] = Array.isArray(cfg.path) && cfg.path.length > 0
            ? cfg.path
            : [{ id: cfg.folder_id, name: cfg.folder_name ?? cfg.folder_id }];
          prefill = {
            accountId: cfg.account_id,
            workspaceId: cfg.workspace_id,
            projectId: cfg.project_id,
            projectName: cfg.project_name ?? "",
            folderPath: path,
          };
          chosenFolder = path.at(-1) ?? null;
          if (!name.trim()) name = dest.display_name ?? "";
        }
      }
    } catch {
      // Prefill is a convenience; fall back to the empty picker.
    } finally {
      prefillLoading = false;
    }
  });

  function chooseFolder() {
    chosenFolder = currentFolder;
    if (!name.trim() && chosenFolder) name = chosenFolder.name;
  }

  async function create() {
    if (!chosenFolder) {
      error = "Pick the folder to watch.";
      return;
    }
    if (!destinationPath.trim()) {
      error = "Set a local destination path.";
      return;
    }
    saving = true;
    error = null;
    try {
      const body = {
        name: name.trim() || chosenFolder.name,
        source: {
          type: "frameio",
          account_id: accountId,
          workspace_id: workspaceId,
          project_id: projectId,
          folder_id: chosenFolder.id,
          recursive,
          // Names + path sent so the backend skips the tightly rate-limited folder lookups.
          folder_name: chosenFolder.name,
          project_name: projectName,
          path: folderPath.at(-1)?.id === chosenFolder.id ? folderPath : null,
        },
        destination_path: destinationPath.trim(),
        conflict_policy: conflictPolicy,
        path_template: pathTemplate.trim() || null,
        enabled,
      };
      const res = await fetch("/api/sync-rules", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) {
        error = (await res.json().catch(() => ({}))).detail ?? `Could not create (${res.status})`;
        return;
      }
      await goto("/sync-rules");
    } finally {
      saving = false;
    }
  }
</script>

<div class="mx-auto max-w-2xl space-y-6">
  <div>
    <a href="/sync-rules" class="text-sm text-muted hover:text-text">← Destinations</a>
    <h1 class="mt-1 text-2xl font-semibold">New sync rule</h1>
  </div>

  {#if error}<p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>{/if}

  <div class="space-y-5 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Source folder</h2>

    {#if !prefillLoading}
      <ProjectFolderBrowser
        bind:accountId
        bind:workspaceId
        bind:projectId
        bind:projectName
        bind:folderPath
        initial={prefill}
      >
        {#snippet crumbAction()}
          <button onclick={chooseFolder} class="ml-auto rounded border border-border bg-surface px-2 py-0.5 text-xs hover:bg-surface-2">Watch this folder</button>
        {/snippet}
      </ProjectFolderBrowser>
    {:else}
      <p class="text-xs text-faint">Loading…</p>
    {/if}

    {#if chosenFolder}
      <div class="flex items-center justify-between rounded-md bg-accent px-3 py-2 text-sm text-on-accent">
        <span>Watching: <span class="font-medium">{chosenFolder.name}</span></span>
        <label class="flex items-center gap-1.5 text-xs text-faint">
          <input type="checkbox" bind:checked={recursive} /> Include subfolders
        </label>
      </div>
    {/if}
  </div>

  <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Options</h2>
    <label class="block text-sm">
      <span class="text-muted">Rule name</span>
      <input bind:value={name} placeholder="defaults to the folder name" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-muted">Destination path (on the Portal host)</span>
      <input bind:value={destinationPath} placeholder="/data/incoming" class="mt-1 w-full rounded-md border border-border px-2 py-1.5 font-mono" />
    </label>
    <label class="block text-sm">
      <span class="text-muted">On filename conflict</span>
      <select bind:value={conflictPolicy} class="mt-1 w-full rounded-md border border-border px-2 py-1.5">
        <option value="rename_suffix">Rename (add suffix)</option>
        <option value="skip">Skip</option>
        <option value="overwrite">Overwrite</option>
      </select>
    </label>
    <div class="block text-sm">
      <span class="text-muted">Path template (optional)</span>
      <TemplateTokenInput bind:value={pathTemplate} tokens={SYNC_TOKENS} placeholder={"{project}/{date}/{filename}"} />
      <p class="mt-1 text-xs text-faint">Leave blank to keep original names at the destination root.</p>
    </div>
    <div class="rounded-md border border-border bg-surface-2 p-3 text-xs text-muted">
      <div class="mb-1 font-medium text-faint">Examples</div>
      <ul class="space-y-0.5 font-mono">
        <li>{"{project}/{date}/{filename}"} → Acme/2026-06-12/clip.mov</li>
        <li>{"{year}/{month}/{filename}"} → 2026/06/clip.mov</li>
        <li>{"{folder}/{stem}{ext}"} → Dailies/clip.mov</li>
      </ul>
    </div>
    <label class="flex items-center gap-1.5 text-sm">
      <input type="checkbox" bind:checked={enabled} /> Enable now (creates the Frame.io webhook)
    </label>
  </div>

  <div class="flex items-center gap-3">
    <button onclick={create} disabled={saving || !chosenFolder} class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50">
      {saving ? "Creating…" : "Create rule"}
    </button>
    <a href="/sync-rules" class="text-sm text-muted hover:text-text">Cancel</a>
  </div>
</div>
