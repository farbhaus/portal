<script lang="ts">
  import { goto } from "$app/navigation";
  import { timeAgo, whenFull } from "$lib/links";
  import type { Activity } from "./+page";

  let { data } = $props();
  const events = $derived(data.events as Activity[]);

  function onFilter(e: Event) {
    const v = (e.target as HTMLSelectElement).value;
    goto(v ? `/activity?action=${encodeURIComponent(v)}` : "/activity");
  }

  // "upload_link.created" → "Upload link created"
  const words = (a: string) => {
    const s = a.replace(/[._]/g, " ").trim();
    return s.charAt(0).toUpperCase() + s.slice(1);
  };
  function detailText(d: Record<string, unknown> | null): string {
    if (!d) return "";
    return Object.entries(d)
      .filter(([, v]) => v != null && v !== "")
      .map(([k, v]) => `${k.replace(/_/g, " ")}: ${typeof v === "object" ? JSON.stringify(v) : v}`)
      .join(" · ");
  }
</script>

<div class="mx-auto max-w-5xl space-y-4 text-[13px]">
  <div>
    <a href="/settings" class="text-muted hover:text-text">← Settings</a>
    <div class="mt-2 flex flex-wrap items-center gap-3">
      <h1 class="flex-1 text-xl font-semibold tracking-tight">Admin log</h1>
      <select onchange={onFilter} class="h-8 rounded-md border border-border bg-bg px-2 text-[13px]" aria-label="Filter by action">
        <option value="" selected={!data.action}>Everything</option>
        {#each data.actions as a (a)}<option value={a} selected={data.action === a}>{words(a)}</option>{/each}
      </select>
    </div>
  </div>

  <div class="overflow-hidden rounded-xl border border-border bg-surface">
    {#each events as ev (ev.id)}
      <div class="grid grid-cols-[minmax(0,1fr)_auto] gap-x-4 gap-y-0.5 border-b border-border px-4 py-2.5 last:border-b-0 sm:grid-cols-[180px_minmax(0,1fr)_110px]">
        <span class="font-medium text-text">{words(ev.action)}</span>
        <span class="order-last col-span-2 truncate font-mono text-[11.5px] text-faint sm:order-none sm:col-span-1" title={detailText(ev.detail)}>{detailText(ev.detail)}{ev.user_email ? ` · by ${ev.user_email}` : ""}</span>
        <span class="text-right text-xs text-faint" title={whenFull(ev.created_at)}>{timeAgo(ev.created_at)}</span>
      </div>
    {:else}
      <p class="px-4 py-10 text-center text-faint">Nothing logged yet.</p>
    {/each}
  </div>
</div>
