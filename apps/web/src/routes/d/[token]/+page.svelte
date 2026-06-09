<script lang="ts">
  import {
    downloadAll,
    downloadFile,
    formatBytes,
    startSession,
    type DownloadFile,
  } from "$lib/download";

  let { data } = $props();
  const link = $derived(data.link);
  const accent = $derived(link.accent_color || "#111111");

  let name = $state("");
  let email = $state("");
  let password = $state("");

  let sessionId = $state<string | null>(null);
  let files = $state<DownloadFile[]>([]);
  let opening = $state(false);
  let error = $state<string | null>(null);
  let busyFile = $state<string | null>(null);
  let allProgress = $state<string | null>(null);

  const needsGate = $derived(
    link.password_required || link.viewer_fields_required.name || link.viewer_fields_required.email,
  );

  function missing(): string | null {
    if (link.viewer_fields_required.name && !name.trim()) return "Please enter your name.";
    if (link.viewer_fields_required.email && !email.trim()) return "Please enter your email.";
    if (link.password_required && !password) return "Please enter the password.";
    return null;
  }

  async function open() {
    const m = missing();
    if (m) {
      error = m;
      return;
    }
    opening = true;
    error = null;
    try {
      const res = await startSession(data.token, {
        name: name || undefined,
        email: email || undefined,
        password: password || undefined,
      });
      sessionId = res.sessionId;
      files = res.files;
    } catch (e) {
      error = e instanceof Error ? e.message : "Could not open this link.";
    } finally {
      opening = false;
    }
  }

  async function one(f: DownloadFile) {
    if (!sessionId) return;
    busyFile = f.id;
    error = null;
    try {
      await downloadFile(data.token, sessionId, f.id);
    } catch (e) {
      error = e instanceof Error ? e.message : "Download failed.";
    } finally {
      busyFile = null;
    }
  }

  async function all() {
    if (!sessionId) return;
    error = null;
    try {
      await downloadAll(data.token, sessionId, files, (done, total) => {
        allProgress = `${done} / ${total}`;
      });
    } catch (e) {
      error = e instanceof Error ? e.message : "Download failed.";
    } finally {
      allProgress = null;
    }
  }
</script>

<svelte:head><title>{link.display_name}</title></svelte:head>

<div class="flex min-h-screen flex-col items-center bg-neutral-50 px-4 py-12">
  <div class="w-full max-w-xl">
    <div class="mb-6 text-center">
      {#if link.logo_url}<img src={link.logo_url} alt="" class="mx-auto mb-4 h-12 object-contain" />{/if}
      <h1 class="text-2xl font-semibold">{link.display_name}</h1>
      {#if link.subtitle}<p class="mt-1 text-neutral-500">{link.subtitle}</p>{/if}
    </div>

    {#if link.state !== "ok"}
      <div class="rounded-xl border border-neutral-200 bg-white p-8 text-center">
        <p class="text-neutral-600">
          {link.state === "expired" ? "This download link has expired." : "This download link is no longer active."}
        </p>
      </div>
    {:else}
      <div class="space-y-5 rounded-xl border border-neutral-200 bg-white p-6">
        {#if error}<p class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">{error}</p>{/if}

        {#if sessionId === null}
          {#if needsGate}
            <div class="space-y-3">
              {#if link.viewer_fields_required.name}
                <input bind:value={name} placeholder="Your name" class="w-full rounded-md border border-neutral-300 px-3 py-2 text-sm" />
              {/if}
              {#if link.viewer_fields_required.email}
                <input bind:value={email} type="email" placeholder="Your email" class="w-full rounded-md border border-neutral-300 px-3 py-2 text-sm" />
              {/if}
              {#if link.password_required}
                <input bind:value={password} type="password" placeholder="Password" class="w-full rounded-md border border-neutral-300 px-3 py-2 text-sm" />
              {/if}
            </div>
          {/if}
          <button onclick={open} disabled={opening} class="w-full rounded-md px-4 py-2.5 text-sm font-medium text-white disabled:opacity-50" style="background:{accent}">
            {opening ? "Opening…" : "View files"}
          </button>
        {:else}
          <div class="flex items-center justify-between">
            <span class="text-sm text-neutral-500">{files.length} file{files.length === 1 ? "" : "s"}</span>
            {#if files.length > 1}
              <button onclick={all} disabled={allProgress !== null} class="rounded-md px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50" style="background:{accent}">
                {allProgress ? `Downloading ${allProgress}…` : "Download all"}
              </button>
            {/if}
          </div>

          {#if files.length === 0}
            <p class="text-sm text-neutral-400">No files to download.</p>
          {:else}
            <div class="divide-y divide-neutral-100">
              {#each files as f (f.id)}
                <div class="flex items-center gap-3 py-2.5">
                  {#if link.allow_preview && f.thumbnail_url}
                    <img src={f.thumbnail_url} alt="" class="h-10 w-14 rounded object-cover" />
                  {:else}
                    <div class="flex h-10 w-14 items-center justify-center rounded bg-neutral-100 text-neutral-400">📄</div>
                  {/if}
                  <div class="min-w-0 flex-1">
                    <div class="truncate text-sm">{f.name}</div>
                    <div class="text-xs text-neutral-400">{formatBytes(f.size)}</div>
                  </div>
                  <button onclick={() => one(f)} disabled={busyFile === f.id} class="rounded-md border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 disabled:opacity-50">
                    {busyFile === f.id ? "…" : "Download"}
                  </button>
                </div>
              {/each}
            </div>
          {/if}
        {/if}
      </div>
    {/if}

    <p class="mt-6 text-center text-xs text-neutral-400">Powered by Portal</p>
  </div>
</div>
