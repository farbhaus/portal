<script lang="ts">
  import { goto } from "$app/navigation";
  import { PathBreadcrumb, StatusPill } from "$lib/components";

  let { data } = $props();
  const dest = $derived(data.destination);

  const path = $derived(
    dest.config.path?.length
      ? dest.config.path
      : [{ name: dest.config.folder_name ?? dest.config.folder_id ?? "folder" }],
  );

  let displayName = $state("");
  let subtitle = $state("");

  // Seed editable fields from the loaded destination, and re-seed if we navigate to a
  // different one (without clobbering in-progress edits to the same destination).
  let seededId = "";
  $effect(() => {
    if (dest.id !== seededId) {
      seededId = dest.id;
      displayName = dest.display_name;
      subtitle = dest.subtitle ?? "";
    }
  });

  let saving = $state(false);
  let error = $state<string | null>(null);
  let saved = $state(false);

  async function save() {
    if (!displayName.trim()) return;
    saving = true;
    error = null;
    saved = false;
    try {
      const res = await fetch(`/api/destinations/${dest.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          display_name: displayName.trim(),
          subtitle: subtitle.trim() || null,
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        error = body.detail ?? `Could not save (${res.status})`;
        return;
      }
      saved = true;
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!confirm(`Delete destination "${dest.display_name}"?`)) return;
    await fetch(`/api/destinations/${dest.id}`, { method: "DELETE" });
    await goto("/destinations");
  }
</script>

<div class="mx-auto max-w-2xl space-y-6">
  <div>
    <a href="/destinations" class="text-sm text-muted hover:text-text">← Destinations</a>
    <h1 class="mt-1 text-2xl font-semibold">Edit destination</h1>
  </div>

  {#if error}
    <p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
  {:else if saved}
    <p class="rounded-md bg-success/10 px-3 py-2 text-sm text-success">Saved.</p>
  {/if}

  <div class="rounded-xl border border-border bg-surface p-6 text-sm">
    <h2 class="font-medium">Frame.io folder</h2>
    <p class="mt-2">
      <PathBreadcrumb segments={path} class="text-sm" />
    </p>
    <p class="mt-1 text-xs text-faint">The folder binding is fixed. Create a new destination to target a different folder.</p>
  </div>

  <div class="rounded-xl border border-border bg-surface p-6 text-sm">
    <div class="flex items-center justify-between">
      <h2 class="font-medium">Sync to local storage</h2>
      <a href="/sync-rules/new?destination={dest.id}" class="text-xs text-accent hover:underline">+ Sync rule</a>
    </div>
    {#if dest.sync_rules.length === 0}
      <p class="mt-2 text-xs text-muted">Not synced — files stay in Frame.io only.</p>
    {:else}
      <div class="mt-2 space-y-1.5">
        {#each dest.sync_rules as sr (sr.id)}
          <div class="flex flex-wrap items-center gap-2 text-xs">
            <a href="/sync-rules/{sr.id}" class="font-medium hover:text-accent">{sr.name}</a>
            <StatusPill status={sr.enabled ? "ok" : "revoked"} label={sr.enabled ? "enabled" : "disabled"} />
            {#if !sr.exact}<span class="text-faint">via parent folder</span>{/if}
            <span class="text-faint">→</span>
            <span class="font-mono text-muted">{sr.destination_path}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <div class="rounded-xl border border-border bg-surface p-6 text-sm">
    <div class="flex items-center justify-between">
      <h2 class="font-medium">Upload links</h2>
      <a href="/links/new?type=upload&destination={dest.id}" class="text-xs text-accent hover:underline">+ Upload link</a>
    </div>
    {#if dest.upload_links.length === 0}
      <p class="mt-2 text-xs text-muted">No upload links deliver here yet.</p>
    {:else}
      <div class="mt-2 space-y-1.5">
        {#each dest.upload_links as link (link.id)}
          <div class="flex items-center justify-between gap-3 text-xs">
            <a href="/upload-links/{link.id}" class="truncate hover:text-accent">{link.label}</a>
            <StatusPill status={link.state} />
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <div class="space-y-4 rounded-xl border border-border bg-surface p-6">
    <h2 class="font-medium">Details</h2>
    <label class="block text-sm">
      <span class="text-muted">Display name <span class="text-danger">*</span></span>
      <input bind:value={displayName} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-muted">Subtitle</span>
      <input bind:value={subtitle} class="mt-1 w-full rounded-md border border-border px-2 py-1.5" />
    </label>
  </div>

  <div class="flex items-center justify-between">
    <div class="flex items-center gap-3">
      <button
        onclick={save}
        disabled={saving || !displayName.trim()}
        class="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50"
      >
        {saving ? "Saving…" : "Save changes"}
      </button>
      <a href="/destinations" class="text-sm text-muted hover:text-text">Cancel</a>
    </div>
    <button onclick={remove} class="text-sm text-danger hover:text-danger">Delete</button>
  </div>
</div>
