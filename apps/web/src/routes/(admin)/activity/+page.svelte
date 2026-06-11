<script lang="ts">
  import { goto } from "$app/navigation";
  import type { Activity } from "./+page.server";

  let { data } = $props();
  const events = $derived(data.events as Activity[]);

  function onFilter(e: Event) {
    const v = (e.target as HTMLSelectElement).value;
    goto(v ? `/activity?action=${encodeURIComponent(v)}` : "/activity");
  }

  function detailText(d: Record<string, unknown> | null): string {
    if (!d) return "";
    return Object.entries(d)
      .map(([k, v]) => `${k}=${v}`)
      .join("  ");
  }
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-semibold">Activity</h1>
    <select onchange={onFilter} class="rounded-md border border-neutral-300 px-2 py-1.5 text-sm">
      <option value="" selected={!data.action}>All actions</option>
      {#each data.actions as a (a)}
        <option value={a} selected={data.action === a}>{a}</option>
      {/each}
    </select>
  </div>

  {#if events.length === 0}
    <div class="rounded-xl border border-dashed border-neutral-300 p-10 text-center text-neutral-500">
      No activity yet.
    </div>
  {:else}
    <div class="overflow-hidden rounded-xl border border-neutral-200 bg-white">
      <table class="w-full text-sm">
        <thead class="border-b border-neutral-200 text-left text-neutral-500">
          <tr>
            <th class="px-4 py-2 font-medium">When</th>
            <th class="px-4 py-2 font-medium">Who</th>
            <th class="px-4 py-2 font-medium">Action</th>
            <th class="px-4 py-2 font-medium">Detail</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-neutral-100">
          {#each events as ev (ev.id)}
            <tr>
              <td class="px-4 py-2 whitespace-nowrap text-neutral-500">{new Date(ev.created_at).toLocaleString()}</td>
              <td class="px-4 py-2 text-neutral-500">{ev.user_email ?? "—"}</td>
              <td class="px-4 py-2"><span class="rounded-full bg-neutral-100 px-2 py-0.5 text-xs">{ev.action}</span></td>
              <td class="px-4 py-2 font-mono text-xs text-neutral-400">{detailText(ev.detail)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
