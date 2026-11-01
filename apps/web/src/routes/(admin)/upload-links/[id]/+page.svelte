<script lang="ts">
  import { LinkComposer } from "$lib/components";
  import { formatBytes } from "$lib/format";
  import { refreshOnChange } from "$lib/live.svelte";
  import { timeAgo, whenFull, type UploadSessionRow } from "$lib/links";
  import { recentFolders } from "$lib/recents";

  let { data } = $props();
  const recents = $derived(recentFolders(data.recentsFrom));
  const sessions = $derived(data.sessions as UploadSessionRow[]);

  $effect(() => refreshOnChange());
</script>

<div class="space-y-6">
  {#key data.link.id}
    <LinkComposer mode="edit" link={data.link} {recents} rules={data.rules} projectFolders={data.projectFolders} branding={data.branding} emailConfigured={data.emailConfigured} presets={data.presets} brands={data.brands} defaultNotify={data.defaultNotify} />
  {/key}

  <section class="overflow-hidden rounded-xl border border-border bg-surface text-[13px] lg:mr-[364px]">
    <div class="flex items-center border-b border-border px-5 py-3">
      <h2 class="flex-1 text-sm font-semibold">Uploads</h2>
      <span class="text-xs text-faint">{sessions.length ? `Latest ${sessions.length}` : ""}</span>
    </div>
    {#each sessions as s (s.id)}
      <div class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-0.5 border-b border-border px-5 py-2.5 last:border-b-0 sm:grid-cols-[minmax(0,1fr)_80px_120px_140px]">
        <div class="min-w-0">
          <div class="truncate font-medium text-text">{s.uploader_name || s.uploader_email || "Someone"}</div>
          {#if s.uploader_name && s.uploader_email}<div class="truncate text-xs text-faint">{s.uploader_email}</div>{/if}
        </div>
        <span class="tnum hidden text-muted sm:block">{s.files} file{s.files === 1 ? "" : "s"}</span>
        <span class="tnum hidden text-muted sm:block">{s.total_bytes != null ? formatBytes(s.total_bytes) : "—"}</span>
        <span class="text-right text-xs {s.status === 'in_progress' ? 'text-accent-hover' : s.status === 'abandoned' ? 'text-warning' : 'text-muted'}" title={whenFull(s.completed_at ?? s.last_activity_at ?? s.started_at)}>
          {s.status === "in_progress" ? "Uploading now" : s.status === "abandoned" ? "Didn't finish" : timeAgo(s.completed_at ?? s.started_at)}
        </span>
      </div>
    {:else}
      <p class="px-5 py-8 text-center text-faint">No uploads yet.</p>
    {/each}
  </section>
</div>
