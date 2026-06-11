<script lang="ts">
  import type { Activity } from "./activity/+page.server";

  let { data } = $props();
  const recent = $derived(data.recent as Activity[]);

  const cards = $derived([
    { title: "Destinations", count: data.summary.destinations, href: "/destinations" },
    { title: "Upload links", count: data.summary.upload_links, href: "/upload-links" },
    { title: "Download links", count: data.summary.download_links, href: "/download-links" },
    { title: "Sync rules", count: data.summary.sync_rules, href: "/sync-rules" },
  ]);
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold">Dashboard</h1>
      <p class="text-sm text-neutral-500">Signed in as {data.user.email}</p>
    </div>
    <a href="/connections" class="rounded-full px-3 py-1 text-xs font-medium {data.connected ? 'bg-green-50 text-green-700' : 'bg-amber-50 text-amber-700'}">
      Frame.io: {data.connected ? "connected" : "not connected"}
    </a>
  </div>

  <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
    {#each cards as card (card.title)}
      <a href={card.href} class="rounded-xl border border-neutral-200 bg-white p-5 hover:border-neutral-400">
        <div class="text-3xl font-semibold">{card.count}</div>
        <div class="mt-1 text-sm text-neutral-500">{card.title}</div>
      </a>
    {/each}
  </div>

  <div class="space-y-3 rounded-xl border border-neutral-200 bg-white p-6">
    <div class="flex items-center justify-between">
      <h2 class="font-medium">Recent activity</h2>
      <a href="/activity" class="text-sm text-neutral-500 hover:text-neutral-900">View all →</a>
    </div>
    {#if recent.length === 0}
      <p class="text-sm text-neutral-400">No activity yet.</p>
    {:else}
      <div class="divide-y divide-neutral-100 text-sm">
        {#each recent as ev (ev.id)}
          <div class="flex items-center justify-between py-2">
            <span><span class="rounded-full bg-neutral-100 px-2 py-0.5 text-xs">{ev.action}</span> <span class="text-neutral-400">{ev.user_email ?? ""}</span></span>
            <span class="text-xs text-neutral-400">{new Date(ev.created_at).toLocaleString()}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
