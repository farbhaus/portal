<script lang="ts">
  import { onMount } from "svelte";
  import { Button, Card, PoweredByPortal } from "$lib/components";
  import {
    downloadAll,
    downloadAllToFolder,
    downloadFile,
    formatBytes,
    requestCode,
    startSession,
    supportsFolderPicker,
    type DownloadFile,
    type FolderProgress,
  } from "$lib/download";

  let { data } = $props();
  const link = $derived(data.link);
  const accent = $derived(link.accent_color || "#f59e0b");
  const inputCls =
    "w-full rounded-md border border-border px-3 py-2 text-sm outline-none focus:border-accent";

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
  // "Download all" progress. `label` + `pct` (null = indeterminate) drive the bar; folder downloads
  // bypass the browser's download manager, so this is the only progress the recipient sees.
  let allProgress = $state<{ label: string; pct: number | null } | null>(null);
  let doneMsg = $state<string | null>(null);
  // Chrome/Edge can stream "Download all" into a folder the recipient picks (preserving the
  // subfolder tree); other browsers fall back to flat per-file downloads. Resolved after mount so
  // it stays false during SSR (no `window`) and doesn't cause a hydration mismatch.
  let canPickFolder = $state(false);
  onMount(() => {
    canPickFolder = supportsFolderPicker();
  });

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

  // Shared wrapper around a "download all" action: clears banners, swallows a cancelled picker,
  // and always clears the progress bar at the end.
  async function runAll(action: () => Promise<void>) {
    if (!sessionId || allProgress) return;
    error = null;
    doneMsg = null;
    try {
      await action();
    } catch (e) {
      // The recipient dismissing the folder picker isn't an error.
      if (!(e instanceof DOMException && e.name === "AbortError"))
        error = e instanceof Error ? e.message : "Download failed.";
    } finally {
      allProgress = null;
    }
  }

  // Chromium only: stream every file into a folder the recipient picks, with real byte progress.
  function allToFolder() {
    return runAll(async () => {
      const onProgress = (p: FolderProgress) => {
        const pct = p.bytesTotal ? Math.min(100, (p.bytesDone / p.bytesTotal) * 100) : null;
        const count = `${p.fileIndex + 1} / ${p.filesTotal}`;
        allProgress = {
          label:
            p.bytesTotal !== null
              ? `Saving ${p.fileName} — ${formatBytes(p.bytesDone)} of ${formatBytes(p.bytesTotal)} (${count})`
              : `Saving ${p.fileName} (${count})`,
          pct,
        };
      };
      const r = await downloadAllToFolder(data.token, sessionId!, files, onProgress);
      if (r.failed.length)
        error = `Couldn't save ${r.failed.length} file${r.failed.length === 1 ? "" : "s"}: ${r.failed.join(", ")}`;
      if (r.saved > 0) doneMsg = `Saved ${r.saved} file${r.saved === 1 ? "" : "s"} to “${r.dirName}”.`;
    });
  }

  // Hand each file to the browser's own download manager (default Downloads folder). Available on
  // every browser; the only path on non-Chromium browsers.
  function allToDownloads() {
    return runAll(async () => {
      await downloadAll(data.token, sessionId!, files, (done, total) => {
        allProgress = { label: `Sent ${done} / ${total} to your browser…`, pct: (done / total) * 100 };
      });
    });
  }
</script>

<svelte:head><title>{link.display_name}</title></svelte:head>

<div class="flex min-h-screen flex-col items-center justify-center bg-bg px-4 py-12">
  <div class="w-full max-w-xl">
    <div class="mb-6 text-center">
      {#if link.logo_url}<img src={link.logo_url} alt="" class="mx-auto mb-4 h-12 object-contain" />{/if}
      <h1 class="text-2xl font-semibold tracking-tight">{link.display_name}</h1>
      {#if link.subtitle}<p class="mt-1 text-muted">{link.subtitle}</p>{/if}
    </div>

    {#if link.state !== "ok"}
      <Card class="text-center">
        <p class="text-muted">
          {link.state === "expired" ? "This download link has expired." : "This download link is no longer active."}
        </p>
      </Card>
    {:else}
      <Card class="space-y-5">
        {#if error}<p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>{/if}

        {#if sessionId === null}
          {#if needsGate}
            <div class="space-y-3">
              {#if link.viewer_fields_required.name}
                <input bind:value={name} placeholder="Your name" class={inputCls} />
              {/if}
              {#if link.viewer_fields_required.email || link.verify_email}
                <input bind:value={email} type="email" placeholder="Your email" disabled={codeSent} class="{inputCls} disabled:bg-surface-2" />
              {/if}
              {#if link.password_required}
                <input bind:value={password} type="password" placeholder="Password" class={inputCls} />
              {/if}
              {#if link.verify_email && codeSent}
                <div>
                  <input bind:value={code} inputmode="numeric" placeholder="Enter the 6-digit code" class="{inputCls} tracking-widest" />
                  <p class="mt-1 text-xs text-muted">We emailed a code to {email}. <button type="button" onclick={resend} class="underline hover:text-text">Resend</button></p>
                </div>
              {/if}
            </div>
          {/if}
          <Button {accent} onclick={open} disabled={opening} class="w-full">
            {opening
              ? link.verify_email && !codeSent
                ? "Sending…"
                : "Opening…"
              : link.verify_email && !codeSent
                ? "Send code"
                : "View files"}
          </Button>
        {:else}
          <div class="flex items-center justify-between gap-2">
            <span class="text-sm text-muted">{files.length} file{files.length === 1 ? "" : "s"}</span>
            {#if files.length > 1}
              <div class="flex items-center gap-2">
                {#if canPickFolder}
                  <!-- Chromium can do both: stream into a chosen folder, or hand off to the
                       browser's download manager like every other browser. -->
                  <Button variant="ghost" size="sm" onclick={allToDownloads} disabled={allProgress !== null}>
                    Download all
                  </Button>
                  <Button {accent} size="sm" onclick={allToFolder} disabled={allProgress !== null}>
                    {allProgress ? "Downloading…" : "Download to folder…"}
                  </Button>
                {:else}
                  <Button {accent} size="sm" onclick={allToDownloads} disabled={allProgress !== null}>
                    {allProgress ? "Downloading…" : "Download all"}
                  </Button>
                {/if}
              </div>
            {/if}
          </div>

          {#if !canPickFolder && files.length > 1}
            <p class="rounded-md bg-info/10 px-3 py-2 text-xs text-info">
              To keep the original folder structure, open this link in a Chromium-based browser
              (Chrome, Edge, Arc, Brave). Other browsers save the files individually.
            </p>
          {/if}

          {#if allProgress}
            <div class="space-y-1.5" role="status" aria-live="polite">
              <div class="h-1.5 overflow-hidden rounded-full bg-surface-2">
                {#if allProgress.pct === null}
                  <div class="h-full w-1/3 animate-pulse rounded-full" style="background:{accent}"></div>
                {:else}
                  <div class="h-full rounded-full transition-[width] duration-150" style="width:{allProgress.pct}%;background:{accent}"></div>
                {/if}
              </div>
              <p class="truncate text-xs text-faint">{allProgress.label}</p>
            </div>
          {:else if doneMsg}
            <p class="rounded-md bg-accent/10 px-3 py-2 text-sm" style="color:{accent}">{doneMsg}</p>
          {/if}

          {#if files.length === 0}
            <p class="text-sm text-faint">No files to download.</p>
          {:else}
            <div class="divide-y divide-border">
              {#each files as f (f.id)}
                <div class="flex items-center gap-3 py-2.5">
                  {#if link.allow_preview && f.thumbnail_url}
                    <img src={f.thumbnail_url} alt="" class="h-10 w-14 rounded object-cover" />
                  {:else}
                    <div class="flex h-10 w-14 items-center justify-center rounded bg-surface-2 text-faint">
                      <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        <path d="M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z"/><polyline points="14 2 14 8 20 8"/>
                      </svg>
                    </div>
                  {/if}
                  <div class="min-w-0 flex-1">
                    <div class="truncate text-sm">{f.name}</div>
                    <div class="text-xs text-faint">{formatBytes(f.size)}</div>
                  </div>
                  <Button variant="ghost" size="sm" onclick={() => one(f)} disabled={busyFile === f.id}>
                    {busyFile === f.id ? "…" : "Download"}
                  </Button>
                </div>
              {/each}
            </div>
          {/if}
        {/if}
      </Card>
    {/if}

    <PoweredByPortal />
  </div>
</div>
