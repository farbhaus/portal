<script lang="ts">
  import { LinkComposer } from "$lib/components";
  import { refreshOnChange } from "$lib/live.svelte";
  import { timeAgo, whenFull, type DownloadEvent } from "$lib/links";
  import { recentFolders } from "$lib/recents";

  let { data } = $props();
  const recents = $derived(recentFolders(data.recentsFrom));
  const events = $derived(data.events as DownloadEvent[]);

  $effect(() => refreshOnChange());
</script>

<div class="space-y-6">
  {#key data.link.id}
    <LinkComposer mode="edit" link={data.link} {recents} rules={data.rules} projectFolders={data.projectFolders} branding={data.branding} emailConfigured={data.emailConfigured} presets={data.presets} brands={data.brands} defaultNotify={data.defaultNotify} />
  {/key}

  <section class="overflow-hidden rounded-xl border border-border bg-surface text-[13px] lg:mr-[364px]">
    <div class="flex items-center gap-3 border-b border-border px-5 py-3">
      <h2 class="flex-1 text-sm font-semibold">Download log</h2>
      <span class="tnum text-xs text-faint">{data.stats.downloads} download{data.stats.downloads === 1 ? "" : "s"} · {data.stats.unique_viewers} {data.stats.unique_viewers === 1 ? "person" : "people"}</span>
    </div>
    {#each events as e, i (i)}
      <div class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-0.5 border-b border-border px-5 py-2.5 last:border-b-0 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)_130px]">
        <div class="min-w-0">
          <div class="truncate font-medium text-text">{e.viewer_name || e.viewer_email || "Someone"}</div>
          <div class="truncate text-xs text-faint">{[e.viewer_name ? e.viewer_email : null, e.ip].filter(Boolean).join(" · ")}</div>
        </div>
        <span class="hidden truncate text-muted sm:block" title={e.file_name ?? e.frameio_file_id}>{e.file_name ?? e.frameio_file_id}{e.completed ? "" : " (started)"}</span>
        <span class="text-right text-xs text-muted" title={whenFull(e.started_at)}>{timeAgo(e.started_at)}</span>
      </div>
    {:else}
      <p class="px-5 py-8 text-center text-faint">Nobody has downloaded anything yet.</p>
    {/each}
  </section>
</div>
