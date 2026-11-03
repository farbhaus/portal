<script lang="ts">
  // Presets saved from the link composer: rename or delete them here.
  import { invalidateAll } from "$app/navigation";
  import { ConfirmButton, Icon, type Preset } from "$lib/components";
  import { errorDetail, toast } from "$lib/toast.svelte";

  let { presets }: { presets: Preset[] } = $props();

  let renaming = $state<string | null>(null);
  let newName = $state("");

  function summary(p: Preset): string {
    const s = p.settings as Record<string, any>;
    const parts: string[] = [];
    parts.push(s.expiry === "never" ? "no expiry" : s.expiry === "1" ? "1 day" : `${s.expiry ?? 7} days`);
    if (s.otp) parts.push("email code");
    if (s.password) parts.push("password");
    if (p.kind === "upload") {
      const t = (s.types as string[] | undefined)?.filter((x) => x !== "any") ?? [];
      parts.push(t.length ? t.map((x) => x[0].toUpperCase() + x.slice(1)).join(", ") : "any file type");
      if (s.template) parts.push(`sorted by ${s.template}`);
    } else {
      if (s.max_downloads) parts.push(`${s.max_downloads} downloads max`);
      if (s.allow_preview === false) parts.push("no thumbnails");
    }
    if (s.remind) parts.push("reminder");
    return parts.join(" · ");
  }

  async function rename(p: Preset) {
    const res = await fetch(`/api/link-presets/${p.id}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: newName }),
    });
    if (!res.ok) return toast(await errorDetail(res, "Could not rename"), { kind: "error" });
    renaming = null;
    await invalidateAll();
  }

  async function remove(p: Preset) {
    const res = await fetch(`/api/link-presets/${p.id}`, { method: "DELETE" });
    if (!res.ok) return toast(await errorDetail(res, "Could not delete"), { kind: "error" });
    toast(`Deleted “${p.name}”`);
    await invalidateAll();
  }
</script>

<div>
  {#each presets as p (p.id)}
    <div class="flex items-center gap-3 border-t border-border px-5 py-2.5 first:border-t-0">
      <span class="grid h-[30px] w-[30px] shrink-0 place-items-center rounded-md border border-border bg-surface-2 text-body" title={p.kind === "upload" ? "Receive" : "Send"}>
        <Icon name={p.kind === "upload" ? "receive" : "send"} />
      </span>
      {#if renaming === p.id}
        <input
          class="h-8 min-w-0 flex-1 rounded-md border border-border bg-bg px-2.5 outline-none focus:border-accent"
          bind:value={newName}
          aria-label="Preset name"
          onkeydown={(e) => {
            if (e.key === "Enter") void rename(p);
            if (e.key === "Escape") renaming = null;
          }}
        />
        <button type="button" class="h-7 rounded-md px-2.5 text-xs font-medium text-text hover:bg-surface-3" onclick={() => rename(p)}>Save</button>
      {:else}
        <div class="min-w-0 flex-1">
          <div class="truncate font-medium text-text">{p.name}</div>
          <div class="truncate text-xs text-faint">{summary(p)}</div>
        </div>
        <button type="button" class="h-7 rounded-md px-2.5 text-xs font-medium text-muted hover:bg-surface-3 hover:text-text" onclick={() => { renaming = p.id; newName = p.name; }}>Rename</button>
      {/if}
      <ConfirmButton label="Delete preset" confirmLabel="Delete" icon="trash" onconfirm={() => remove(p)} />
    </div>
  {:else}
    <p class="px-5 py-4 text-muted">No presets yet.</p>
  {/each}
</div>
