<script lang="ts">
  import { onMount } from "svelte";
  import { Button, Card, PathBreadcrumb, PoweredByPortal } from "$lib/components";
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
  import { childFolders, crumbs, filesIn, hasFolders, subtree } from "$lib/filetree";
  import {
    downloadAllAsZip,
    isZipUnavailable,
    MAX_ZIP_ENTRIES,
    safeZipName,
    zipSupported,
    type ZipProgress,
  } from "$lib/zipdownload";

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
  // A ZIP is assembled live by our service worker, so leaving the page cancels it (see the guard
  // below); the other two paths don't need the tab to stay open.
  let zipping = $state(false);
  // Live while a "download all" runs, so the recipient can stop it from the page.
  let aborter = $state<AbortController | null>(null);
  // Three ways to keep the folder structure, best first: Chrome/Edge stream into a folder the
  // recipient picks; everyone else gets a ZIP assembled by our service worker; if neither is
  // available it's flat per-file downloads. Both flags resolve after mount so they stay false
  // during SSR (no `window`) and don't cause a hydration mismatch.
  let canPickFolder = $state(false);
  let zipReady = $state(false);
  onMount(async () => {
    canPickFolder = supportsFolderPicker();
    // Registers the worker up front so it is already warm when the recipient clicks.
    zipReady = await zipSupported();
  });

  // Folder browsing. A recursive folder source gives every file a relative `path`, which we turn
  // into a navigable tree; flat sources (single file, curated selection, non-recursive folder) have
  // no paths at all and keep rendering as the plain, source-ordered list.
  let cwd = $state("");
  const tree = $derived(hasFolders(files));
  const folders = $derived(tree ? childFolders(files, cwd) : []);
  // Rows in the current folder, and everything at or below it (what the "download all" buttons act
  // on, with paths relative to `cwd` so a saved folder mirrors what's on screen).
  const rows = $derived(tree ? filesIn(files, cwd) : files);
  const scoped = $derived(tree ? subtree(files, cwd) : files);
  const path = $derived(crumbs(cwd, "All files"));
  const canZip = $derived(zipReady && scoped.length <= MAX_ZIP_ENTRIES);
  // The archive is named after what the recipient is looking at: the folder they're in, or the link.
  const zipName = $derived(safeZipName(cwd ? (path.at(-1)?.name ?? "") : link.display_name));

  function openFolder(folderPath: string) {
    cwd = folderPath;
    error = null;
    doneMsg = null;
  }

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
  async function runAll(action: (signal: AbortSignal) => Promise<void>) {
    if (!sessionId || allProgress) return;
    error = null;
    doneMsg = null;
    aborter = new AbortController();
    try {
      await action(aborter.signal);
    } catch (e) {
      // The recipient dismissing the folder picker isn't an error.
      if (!(e instanceof DOMException && e.name === "AbortError"))
        error = e instanceof Error ? e.message : "Download failed.";
    } finally {
      allProgress = null;
      aborter = null;
    }
  }

  // Stop whatever "download all" is running. Every path takes the same signal, so one button ends
  // any of them and hands the controls straight back — no page reload needed.
  function cancelAll() {
    aborter?.abort();
  }

  // Chromium only: stream every file into a folder the recipient picks, with real byte progress.
  function allToFolder() {
    return runAll(async (signal) => {
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
      const r = await downloadAllToFolder(data.token, sessionId!, scoped, onProgress, signal);
      if (r.failed.length)
        error = `Couldn't save ${r.failed.length} file${r.failed.length === 1 ? "" : "s"}: ${r.failed.join(", ")}`;
      if (r.cancelled)
        doneMsg = `Stopped — ${r.saved} of ${scoped.length} file${scoped.length === 1 ? "" : "s"} saved to “${r.dirName}”.`;
      else if (r.saved > 0)
        doneMsg = `Saved ${r.saved} file${r.saved === 1 ? "" : "s"} to “${r.dirName}”.`;
    });
  }

  // Stream one ZIP through the service worker, keeping the folder tree. The browser's own download
  // manager writes it to disk (and shows the authoritative progress); our bar is the secondary
  // signal, and the only one during the first mint before any bytes move.
  function allToZip() {
    zipping = true;
    return runAll(async (signal) => {
      const onProgress = (p: ZipProgress) => {
        const pct = p.bytesTotal ? Math.min(100, (p.bytesDone / p.bytesTotal) * 100) : null;
        const count = `${p.fileIndex + 1} / ${p.filesTotal}`;
        allProgress = {
          label:
            p.bytesTotal !== null
              ? `Zipping ${p.fileName} — ${formatBytes(p.bytesDone)} of ${formatBytes(p.bytesTotal)} (${count})`
              : `Zipping ${p.fileName} (${count})`,
          pct,
        };
      };
      let r: Awaited<ReturnType<typeof downloadAllAsZip>>;
      try {
        r = await downloadAllAsZip(data.token, sessionId!, scoped, zipName, onProgress, signal);
      } catch (e) {
        // The archive never started — the capability probe should have caught this, so it means the
        // worker went away between the probe and the click. Drop to the per-file path rather than
        // leaving the recipient staring at a button that does nothing.
        if (!isZipUnavailable(e)) throw e;
        zipReady = false;
        error = "Couldn't prepare the archive here. Use “Download all” instead.";
        return;
      }
      // Cancelling leaves a partial .zip in the browser's downloads — say so, because the file is
      // there and looks plausible.
      if (r.cancelled) {
        doneMsg = "Download stopped. Any part-downloaded .zip in your downloads is incomplete.";
        return;
      }
      if (r.failed.length)
        error = `${r.failed.length} file${r.failed.length === 1 ? "" : "s"} couldn't be added; see _FAILED_FILES.txt in the archive.`;
      doneMsg = `Saved ${r.saved} file${r.saved === 1 ? "" : "s"} to ${zipName}.zip.`;
    }).finally(() => (zipping = false));
  }

  // Closing the tab tears down the worker feeding the archive, so warn first. Only for the ZIP
  // path — the other two survive (or never left) the browser's own download manager.
  $effect(() => {
    if (!zipping) return;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  });

  // Hand each file to the browser's own download manager (default Downloads folder). Available on
  // every browser; the last resort when neither of the folder-preserving paths is available.
  function allToDownloads() {
    return runAll(async (signal) => {
      const r = await downloadAll(
        data.token,
        sessionId!,
        scoped,
        (done, total) => {
          allProgress = {
            label: `Sent ${done} / ${total} to your browser…`,
            pct: (done / total) * 100,
          };
        },
        signal,
      );
      if (r.cancelled) doneMsg = "Stopped sending files to your browser.";
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
          {#if tree}
            <PathBreadcrumb
              segments={path}
              onnavigate={(i) => openFolder(path[i].path)}
              class="text-sm"
            />
          {/if}

          <div class="flex items-center justify-between gap-2">
            <span class="text-sm text-muted">
              {scoped.length} file{scoped.length === 1 ? "" : "s"}{cwd ? " in this folder" : ""}
            </span>
            {#if scoped.length > 1}
              <!-- Both folder-preserving paths are offered where available. The ZIP is worth having
                   even on Chromium: it needs no picker and avoids Chrome's "allow multiple
                   downloads?" prompt that the per-file path triggers. -->
              <div class="flex items-center gap-2">
                {#if canZip}
                  <Button
                    variant={canPickFolder ? "ghost" : "primary"}
                    {accent}
                    size="sm"
                    onclick={allToZip}
                    disabled={allProgress !== null}
                  >
                    {allProgress ? "Downloading…" : "Download .zip"}
                  </Button>
                {:else}
                  <Button
                    variant={canPickFolder ? "ghost" : "primary"}
                    {accent}
                    size="sm"
                    onclick={allToDownloads}
                    disabled={allProgress !== null}
                  >
                    {allProgress && !canPickFolder
                      ? "Downloading…"
                      : cwd
                        ? "Download folder"
                        : "Download all"}
                  </Button>
                {/if}
                {#if canPickFolder}
                  <Button {accent} size="sm" onclick={allToFolder} disabled={allProgress !== null}>
                    {allProgress ? "Downloading…" : "Download to folder…"}
                  </Button>
                {/if}
              </div>
            {/if}
          </div>

          {#if allProgress}
            <div class="space-y-1.5" role="status" aria-live="polite">
              <div class="h-1.5 overflow-hidden rounded-full bg-surface-2">
                {#if allProgress.pct === null}
                  <div class="h-full w-1/3 animate-pulse rounded-full" style="background:{accent}"></div>
                {:else}
                  <div class="h-full rounded-full transition-[width] duration-150" style="width:{allProgress.pct}%;background:{accent}"></div>
                {/if}
              </div>
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0">
                  <p class="truncate text-xs text-faint">{allProgress.label}</p>
                  {#if zipping}
                    <p class="text-xs text-faint">
                      Keep this tab open until the archive finishes saving.
                    </p>
                  {/if}
                </div>
                <Button variant="subtle" size="sm" onclick={cancelAll} class="shrink-0">Cancel</Button>
              </div>
            </div>
          {:else if doneMsg}
            <p class="rounded-md bg-accent/10 px-3 py-2 text-sm" style="color:{accent}">{doneMsg}</p>
          {/if}

          {#if files.length === 0}
            <p class="text-sm text-faint">No files to download.</p>
          {:else}
            <div class="divide-y divide-border">
              {#each folders as d (d.path)}
                <button
                  type="button"
                  onclick={() => openFolder(d.path)}
                  class="flex w-full items-center gap-3 py-2.5 text-left transition-colors hover:bg-surface-2"
                >
                  <div class="flex h-10 w-14 items-center justify-center rounded bg-surface-2 text-faint">
                    <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>
                    </svg>
                  </div>
                  <div class="min-w-0 flex-1">
                    <div class="truncate text-sm">{d.name}</div>
                    <div class="text-xs text-faint">
                      {d.fileCount} file{d.fileCount === 1 ? "" : "s"}{d.size === null ? "" : ` · ${formatBytes(d.size)}`}
                    </div>
                  </div>
                  <svg class="h-4 w-4 shrink-0 text-faint" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <polyline points="9 18 15 12 9 6"/>
                  </svg>
                </button>
              {/each}
              {#each rows as f (f.id)}
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
