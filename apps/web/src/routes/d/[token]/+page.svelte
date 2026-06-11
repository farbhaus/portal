<script lang="ts">
  import {
    downloadAll,
    downloadFile,
    formatBytes,
    requestCode,
    startSession,
    type DownloadFile,
  } from "$lib/download";

  let { data } = $props();
  const link = $derived(data.link);
  const accent = $derived(link.accent_color || "#111111");

  let name = $state("");
  let email = $state("");
  let password = $state("");
  let code = $state("");
  let codeSent = $state(false);

  let sessionId = $state<string | null>(null);
  let files = $state<DownloadFile[]>([]);
  let opening = $state(false);
  let error = $state<string | null>(null);
  let busyFile = $state<string | null>(null);
  let allProgress = $state<string | null>(null);

  const needsGate = $derived(
    link.password_required ||
      link.verify_email ||
      link.viewer_fields_required.name ||
      link.viewer_fields_required.email,
  );

  function missing(): string | null {
    if (link.viewer_fields_required.name && !name.trim()) return "Please enter your name.";
    if ((link.viewer_fields_required.email || link.verify_email) && !email.trim())
      return "Please enter your email.";
    if (link.password_required && !password) return "Please enter the password.";
    return null;
  }

  // For verified links: first click emails a code (unless this device is already trusted), the
  // second click (with the code) opens the link. Non-verified links open in one click.
  async function open() {
    const m = missing();
    if (m) {
      error = m;
      return;
    }
    error = null;
    if (link.verify_email && !codeSent) {
      opening = true;
      try {
        const r = await requestCode(data.token, email);
        if (!r.trusted) {
          codeSent = true;
          return;
        }
      } catch (e) {
        error = e instanceof Error ? e.message : "Could not send a code.";
        return;
      } finally {
        opening = false;
      }
    }
    if (link.verify_email && codeSent && !code.trim()) {
      error = "Enter the code we emailed you.";
      return;
    }
    opening = true;
    try {
      const res = await startSession(data.token, {
        name: name || undefined,
        email: email || undefined,
        password: password || undefined,
        code: code || undefined,
      });
      sessionId = res.sessionId;
      files = res.files;
      code = ""; // consumed
    } catch (e) {
      error = e instanceof Error ? e.message : "Could not open this link.";
    } finally {
      opening = false;
    }
  }

  async function resend() {
    error = null;
    try {
      await requestCode(data.token, email);
    } catch (e) {
      error = e instanceof Error ? e.message : "Could not resend the code.";
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

<div class="flex min-h-screen flex-col items-center bg-surface-2 px-4 py-12">
  <div class="w-full max-w-xl">
    <div class="mb-6 text-center">
      {#if link.logo_url}<img src={link.logo_url} alt="" class="mx-auto mb-4 h-12 object-contain" />{/if}
      <h1 class="text-2xl font-semibold">{link.display_name}</h1>
      {#if link.subtitle}<p class="mt-1 text-muted">{link.subtitle}</p>{/if}
    </div>

    {#if link.state !== "ok"}
      <div class="rounded-xl border border-border bg-surface p-8 text-center">
        <p class="text-muted">
          {link.state === "expired" ? "This download link has expired." : "This download link is no longer active."}
        </p>
      </div>
    {:else}
      <div class="space-y-5 rounded-xl border border-border bg-surface p-6">
        {#if error}<p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>{/if}

        {#if sessionId === null}
          {#if needsGate}
            <div class="space-y-3">
              {#if link.viewer_fields_required.name}
                <input bind:value={name} placeholder="Your name" class="w-full rounded-md border border-border px-3 py-2 text-sm" />
              {/if}
              {#if link.viewer_fields_required.email || link.verify_email}
                <input bind:value={email} type="email" placeholder="Your email" disabled={codeSent} class="w-full rounded-md border border-border px-3 py-2 text-sm disabled:bg-surface-2" />
              {/if}
              {#if link.password_required}
                <input bind:value={password} type="password" placeholder="Password" class="w-full rounded-md border border-border px-3 py-2 text-sm" />
              {/if}
              {#if link.verify_email && codeSent}
                <div>
                  <input bind:value={code} inputmode="numeric" placeholder="Enter the 6-digit code" class="w-full rounded-md border border-border px-3 py-2 text-sm tracking-widest" />
                  <p class="mt-1 text-xs text-muted">We emailed a code to {email}. <button type="button" onclick={resend} class="underline hover:text-text">Resend</button></p>
                </div>
              {/if}
            </div>
          {/if}
          <button onclick={open} disabled={opening} class="w-full rounded-md px-4 py-2.5 text-sm font-medium text-on-accent disabled:opacity-50" style="background:{accent}">
            {opening
              ? link.verify_email && !codeSent
                ? "Sending…"
                : "Opening…"
              : link.verify_email && !codeSent
                ? "Send code"
                : "View files"}
          </button>
        {:else}
          <div class="flex items-center justify-between">
            <span class="text-sm text-muted">{files.length} file{files.length === 1 ? "" : "s"}</span>
            {#if files.length > 1}
              <button onclick={all} disabled={allProgress !== null} class="rounded-md px-3 py-1.5 text-sm font-medium text-on-accent disabled:opacity-50" style="background:{accent}">
                {allProgress ? `Downloading ${allProgress}…` : "Download all"}
              </button>
            {/if}
          </div>

          {#if files.length === 0}
            <p class="text-sm text-faint">No files to download.</p>
          {:else}
            <div class="divide-y divide-border">
              {#each files as f (f.id)}
                <div class="flex items-center gap-3 py-2.5">
                  {#if link.allow_preview && f.thumbnail_url}
                    <img src={f.thumbnail_url} alt="" class="h-10 w-14 rounded object-cover" />
                  {:else}
                    <div class="flex h-10 w-14 items-center justify-center rounded bg-surface-2 text-faint">📄</div>
                  {/if}
                  <div class="min-w-0 flex-1">
                    <div class="truncate text-sm">{f.name}</div>
                    <div class="text-xs text-faint">{formatBytes(f.size)}</div>
                  </div>
                  <button onclick={() => one(f)} disabled={busyFile === f.id} class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-2 disabled:opacity-50">
                    {busyFile === f.id ? "…" : "Download"}
                  </button>
                </div>
              {/each}
            </div>
          {/if}
        {/if}
      </div>
    {/if}

    <p class="mt-6 text-center text-xs text-faint">Powered by Portal</p>
  </div>
</div>
