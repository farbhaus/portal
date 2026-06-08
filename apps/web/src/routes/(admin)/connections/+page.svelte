<script lang="ts">
  import { goto, invalidateAll } from "$app/navigation";
  import { page } from "$app/state";

  let { data } = $props();
  let disconnecting = $state(false);

  const flash = $derived(page.url.searchParams.get("frameio"));

  async function disconnect() {
    if (!confirm("Disconnect this Frame.io account?")) return;
    disconnecting = true;
    try {
      await fetch("/api/frameio/disconnect", { method: "POST" });
      await goto("/connections", { replaceState: true });
      await invalidateAll();
    } finally {
      disconnecting = false;
    }
  }

  function fmt(iso: string | null | undefined): string {
    if (!iso) return "—";
    return new Date(iso).toLocaleString();
  }
</script>

<div class="max-w-2xl space-y-6">
  <div>
    <h1 class="text-2xl font-semibold">Connections</h1>
    <p class="text-sm text-neutral-500">Authorize Portal to access your Frame.io account.</p>
  </div>

  {#if flash === "connected"}
    <p class="rounded-md bg-green-50 px-3 py-2 text-sm text-green-700">Frame.io connected.</p>
  {:else if flash === "error"}
    <p class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">
      Frame.io connection failed. Please try again.
    </p>
  {/if}

  <div class="rounded-xl border border-neutral-200 bg-white p-6">
    <div class="flex items-center justify-between">
      <h2 class="font-medium">Frame.io</h2>
      <span
        class="rounded-full px-2.5 py-0.5 text-xs font-medium {data.status.connected
          ? 'bg-green-100 text-green-800'
          : 'bg-neutral-100 text-neutral-600'}"
      >
        {data.status.connected ? "Connected" : "Not connected"}
      </span>
    </div>

    {#if data.status.connected}
      <dl class="mt-4 space-y-2 text-sm">
        <div class="flex justify-between">
          <dt class="text-neutral-500">Account</dt>
          <dd>{data.status.adobe_email ?? "—"}</dd>
        </div>
        <div class="flex justify-between">
          <dt class="text-neutral-500">Token expires</dt>
          <dd>{fmt(data.status.expires_at)}</dd>
        </div>
        <div class="flex justify-between">
          <dt class="text-neutral-500">Token health</dt>
          <dd>{data.status.needs_refresh ? "Refresh due" : "Healthy"}</dd>
        </div>
        <div class="flex justify-between gap-6">
          <dt class="text-neutral-500">Scopes</dt>
          <dd class="text-right text-xs text-neutral-500">{data.status.scopes ?? "—"}</dd>
        </div>
      </dl>
      <button
        onclick={disconnect}
        disabled={disconnecting}
        class="mt-5 rounded-md border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 disabled:opacity-50"
      >
        {disconnecting ? "Disconnecting…" : "Disconnect"}
      </button>
    {:else}
      <p class="mt-2 text-sm text-neutral-500">
        Connect your Frame.io account to create destinations and sync rules.
      </p>
      <a
        href="/api/frameio/oauth/connect"
        data-sveltekit-reload
        class="mt-5 inline-block rounded-md bg-neutral-900 px-4 py-2 text-sm font-medium text-white hover:bg-neutral-800"
      >
        Connect Frame.io
      </a>
    {/if}
  </div>
</div>
