<script lang="ts">
  import { goto } from "$app/navigation";

  let { data } = $props();
  const dest = $derived(data.destination);

  let displayName = $state("");
  let subtitle = $state("");
  let logoUrl = $state("");
  let accentColor = $state("#111111");

  // Seed editable fields from the loaded destination, and re-seed if we navigate to a
  // different one (without clobbering in-progress edits to the same destination).
  let seededId = "";
  $effect(() => {
    if (dest.id !== seededId) {
      seededId = dest.id;
      displayName = dest.display_name;
      subtitle = dest.subtitle ?? "";
      logoUrl = dest.logo_url ?? "";
      accentColor = dest.accent_color ?? "#111111";
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
          logo_url: logoUrl.trim() || null,
          accent_color: accentColor || null,
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

<div class="max-w-2xl space-y-6">
  <div>
    <a href="/destinations" class="text-sm text-neutral-500 hover:text-neutral-900">← Destinations</a>
    <h1 class="mt-1 text-2xl font-semibold">Edit destination</h1>
  </div>

  {#if error}
    <p class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">{error}</p>
  {:else if saved}
    <p class="rounded-md bg-green-50 px-3 py-2 text-sm text-green-700">Saved.</p>
  {/if}

  <div class="rounded-xl border border-neutral-200 bg-white p-6 text-sm">
    <h2 class="font-medium">Frame.io folder</h2>
    <p class="mt-2 text-neutral-600">
      {dest.config.folder_name ?? dest.config.folder_id}
    </p>
    <p class="mt-1 text-xs text-neutral-400">The folder binding is fixed. Create a new destination to target a different folder.</p>
  </div>

  <div class="space-y-4 rounded-xl border border-neutral-200 bg-white p-6">
    <h2 class="font-medium">Branding</h2>
    <label class="block text-sm">
      <span class="text-neutral-500">Display name <span class="text-red-500">*</span></span>
      <input bind:value={displayName} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-neutral-500">Subtitle</span>
      <input bind:value={subtitle} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-neutral-500">Logo URL</span>
      <input bind:value={logoUrl} class="mt-1 w-full rounded-md border border-neutral-300 px-2 py-1.5" />
    </label>
    <label class="block text-sm">
      <span class="text-neutral-500">Accent color</span>
      <input type="color" bind:value={accentColor} class="mt-1 block h-9 w-16 rounded-md border border-neutral-300" />
    </label>
  </div>

  <div class="flex items-center justify-between">
    <div class="flex items-center gap-3">
      <button
        onclick={save}
        disabled={saving || !displayName.trim()}
        class="rounded-md bg-neutral-900 px-4 py-2 text-sm font-medium text-white hover:bg-neutral-800 disabled:opacity-50"
      >
        {saving ? "Saving…" : "Save changes"}
      </button>
      <a href="/destinations" class="text-sm text-neutral-500 hover:text-neutral-900">Cancel</a>
    </div>
    <button onclick={remove} class="text-sm text-red-600 hover:text-red-700">Delete</button>
  </div>
</div>
