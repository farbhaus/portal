<script lang="ts">
  import { goto } from "$app/navigation";
  import { ProjectFolderBrowser, type FolderItem } from "$lib/components";

  let accountId = $state("");
  let workspaceId = $state("");
  let projectId = $state("");
  let projectName = $state("");
  // Breadcrumb from the project root down to the currently-browsed folder (the target).
  let folderPath = $state<FolderItem[]>([]);

  let displayName = $state("");
  let subtitle = $state("");

  let error = $state<string | null>(null);
  let saving = $state(false);

  const currentFolder = $derived(folderPath.at(-1) ?? null);

  async function save() {
    if (!displayName.trim() || !currentFolder) return;
    saving = true;
    error = null;
    try {
      const res = await fetch("/api/destinations", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          display_name: displayName.trim(),
          config: {
            type: "frameio",
            account_id: accountId,
            workspace_id: workspaceId,
            project_id: projectId,
            folder_id: currentFolder.id,
            // Sent so the backend caches the display breadcrumb without extra Frame.io calls.
            path: folderPath,
            project_name: projectName || null,
          },
          subtitle: subtitle.trim() || null,
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        error = body.detail ?? `Could not create destination (${res.status})`;
        return;
      }
      await goto("/destinations");
    } finally {
      saving = false;
    }
  }
</script>

<div class="mx-auto max-w-2xl space-y-6">
  <div>
    <a href="/destinations" class="text-sm text-muted hover:text-text">← Destinations</a>
    <h1 class="mt-1 text-2xl font-semibold">New destination</h1>
  </div>

  {#if error}
    <p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
  {/if}

  <div class="space-y-5 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Frame.io folder</h2>

    <ProjectFolderBrowser
      bind:accountId
      bind:workspaceId
      bind:projectId
      bind:projectName
      bind:folderPath
    />

    {#if currentFolder}
      <p class="text-xs text-muted">
        Target folder: <span class="font-medium">{currentFolder.name}</span>
      </p>
    {/if}
  </div>

  <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Details</h2>
    <label class="block text-sm">
      <span class="text-muted">Display name <span class="text-danger">*</span></span>
      <input bind:value={displayName} placeholder="e.g. DIT Drop — Project X" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-muted">Subtitle</span>
      <input bind:value={subtitle} placeholder="Shown on the upload page" class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
    </label>
  </div>

  <div class="flex items-center gap-3">
    <button
      onclick={save}
      disabled={saving || !displayName.trim() || !currentFolder}
      class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50"
    >
      {saving ? "Creating…" : "Create destination"}
    </button>
    <a href="/destinations" class="text-sm text-muted hover:text-text">Cancel</a>
  </div>
</div>
