<script lang="ts">
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { Button, Card, CopyButton, PoweredByPortal } from "$lib/components";
  import { expiryDate, formatBytes } from "$lib/format";
  import {
    abandonSession,
    clearPendingUpload,
    fileExtension,
    fileKey,
    heartbeat,
    itemsFromDataTransfer,
    itemsFromFileList,
    loadPendingUpload,
    requestCode,
    runUpload,
    savePendingUpload,
    startSession,
    verifyCode,
    type FilePatch,
    type PendingSummary,
    type UploadItem,
  } from "$lib/upload";
  import { loadUploaderPrefs, saveUploaderPrefs } from "$lib/uploader-prefs";

  let { data } = $props();
  const link = $derived(data.link);
  const accent = $derived(link.accent_color || "#f59e0b");
  const inputCls =
    "w-full rounded-md border border-border px-3 py-2 text-sm outline-none focus:border-accent";

  let name = $state("");
  let email = $state("");
  let message = $state("");
  let project = $state("");
  let password = $state("");
  let code = $state("");
  let codeSent = $state(false);
  let verified = $state(false); // device already trusted; no code needed
  let sendingCode = $state(false);
  let verifying = $state(false);

  // The uploader is gated until the email is verified (for verify_email links).
  const locked = $derived(link.verify_email && !verified);

  // An upload this browser started and didn't finish (a closed tab, a crash, a reload).
  let pending = $state<PendingSummary | null>(null);
  const pendingTotal = $derived(pending ? pending.files.reduce((n, f) => n + f.size, 0) : 0);

  // Prefill for returning uploaders. Never touches `verified` — a verify_email
  // link still gates on the OTP / device-trust cookie exactly as for a blank form.
  onMount(() => {
    void loadPendingUpload(data.token).then((p) => (pending = p));
    const prefs = loadUploaderPrefs();
    if (!prefs) return;
    if (!name) name = prefs.name;
    if (!email) email = prefs.email;
  });

  async function discardPending() {
    if (pending) abandonSession(data.token, pending.sessionId);
    await clearPendingUpload(data.token);
    pending = null;
  }

  let items = $state<UploadItem[]>([]);
  let dragging = $state(false);
  let filesInput = $state<HTMLInputElement>();
  let folderInput = $state<HTMLInputElement>();
  let phase = $state<"select" | "uploading" | "done" | "error">("select");
  let error = $state<string | null>(null);
  let sessionId = $state<string | null>(null);
  let uploadedBytes = $state(0);
  // Per file: where it's at, how much has gone, how often a chunk had to be retried.
  type FileView = { state: "queued" | "uploading" | "retrying" | "verifying" | "done" | "failed"; bytes: number; retries: number; error?: string };
  let fileViews = $state<Record<string, FileView>>({});
  const doneCount = $derived(Object.values(fileViews).filter((f) => f.state === "done").length);
  // Speed: a smoothed average of the last few seconds, for the overall line and time left.
  let speed = $state(0);
  let lastSample = { t: 0, bytes: 0 };
  let deliveredAt = $state<Date | null>(null);
  let receiptTo = $state<string | null>(null);

  const totalBytes = $derived(items.reduce((s, it) => s + it.file.size, 0));

  // A receipt of what arrived, for the uploader to keep or forward (issue #86).
  const deliveredLabel = $derived(
    deliveredAt?.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) ?? "",
  );
  const receipt = $derived(
    [
      `Delivered to ${link.display_name} — ${deliveredLabel}`,
      `${items.length} file${items.length === 1 ? "" : "s"} (${formatBytes(totalBytes)})`,
      "",
      ...items.map((it) => `${it.path}  (${formatBytes(it.file.size)})`),
    ].join("\n"),
  );
  const percent = $derived(totalBytes > 0 ? Math.min(100, (uploadedBytes / totalBytes) * 100) : 0);
  const etaText = $derived.by(() => {
    if (speed <= 0 || uploadedBytes >= totalBytes) return "";
    const s = Math.round((totalBytes - uploadedBytes) / speed);
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    if (h) return `${h}h ${String(m).padStart(2, "0")}m left`;
    if (m) return `${m}m ${String(s % 60).padStart(2, "0")}s left`;
    return `${s}s left`;
  });

  // Liveness for the server while uploading: a progress heartbeat, plus a best-effort abandon
  // beacon if the tab goes away mid-upload — so the admin dashboard never shows ghost uploads.
  // Reading uploadedBytes inside the interval callback is untracked, so the effect only re-runs
  // on phase/session changes, and its cleanup drops the pagehide listener once the upload ends.
  $effect(() => {
    if (phase !== "uploading" || !sessionId) return;
    const sid = sessionId;
    const t = setInterval(() => heartbeat(data.token, sid, uploadedBytes), 5_000);
    lastSample = { t: Date.now(), bytes: uploadedBytes };
    const sampler = setInterval(() => {
      const now = Date.now();
      const rate = ((uploadedBytes - lastSample.bytes) * 1000) / Math.max(1, now - lastSample.t);
      speed = speed ? speed * 0.7 + rate * 0.3 : rate;
      lastSample = { t: now, bytes: uploadedBytes };
    }, 1000);
    const onPageHide = () => abandonSession(data.token, sid);
    window.addEventListener("pagehide", onPageHide);
    return () => {
      clearInterval(t);
      clearInterval(sampler);
      window.removeEventListener("pagehide", onPageHide);
    };
  });

  function validate(newItems: UploadItem[]): string | null {
    for (const it of newItems) {
      if (link.allowed_extensions && !link.allowed_extensions.includes(fileExtension(it.path)))
        return `"${it.path}" has a file type that isn't allowed here.`;
      if (link.max_file_size && it.file.size > link.max_file_size)
        return `"${it.path}" is larger than the ${formatBytes(link.max_file_size)} limit.`;
    }
    return null;
  }

  function addItems(newItems: UploadItem[]) {
    const err = validate(newItems);
    if (err) {
      error = err;
      return;
    }
    error = null;
    // De-dupe by path.
    const byPath = new Map(items.map((it) => [it.path, it]));
    for (const it of newItems) byPath.set(it.path, it);
    items = [...byPath.values()];
  }

  async function onDrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    if (e.dataTransfer) addItems(await itemsFromDataTransfer(e.dataTransfer));
  }

  function onPick(e: Event) {
    const input = e.target as HTMLInputElement;
    if (input.files) addItems(itemsFromFileList(input.files));
    input.value = "";
  }

  function removeItem(path: string) {
    items = items.filter((it) => it.path !== path);
  }

  function missingFields(): string | null {
    const r = link.uploader_fields_required;
    if (r.name && !name.trim()) return "Please enter your name.";
    if (r.email && !email.trim()) return "Please enter your email.";
    if (r.project && !project.trim()) return "Please enter the project.";
    if (r.message && !message.trim()) return "Please enter a message.";
    if (link.password_required && !password) return "Please enter the password.";
    return null;
  }

  async function sendCode() {
    if (!email.trim()) {
      error = "Please enter your email.";
      return;
    }
    sendingCode = true;
    error = null;
    try {
      const r = await requestCode(data.token, email);
      if (r.trusted) verified = true;
      else codeSent = true;
    } catch (e) {
      error = e instanceof Error ? e.message : "Could not send a code.";
    } finally {
      sendingCode = false;
    }
  }

  // Validate the code now so the gate can slide away and reveal the uploader.
  async function verifyAndUnlock() {
    if (!code.trim()) {
      error = "Enter the code we emailed you.";
      return;
    }
    verifying = true;
    error = null;
    try {
      await verifyCode(data.token, email, code);
      verified = true; // gate slides away; device now trusted for the upload
      code = "";
    } catch (e) {
      error = e instanceof Error ? e.message : "That code is incorrect or expired.";
    } finally {
      verifying = false;
    }
  }

  async function start() {
    const fieldErr = missingFields();
    if (fieldErr) {
      error = fieldErr;
      return;
    }
    if (link.verify_email && !verified && !code.trim()) {
      error = "Enter the code we emailed you.";
      return;
    }
    if (items.length === 0) return;
    error = null;
    phase = "uploading";
    sessionId = null;
    uploadedBytes = 0;
    speed = 0;
    fileViews = Object.fromEntries(items.map((it) => [it.path, { state: "queued", bytes: 0, retries: 0 } as FileView]));
    try {
      // Picking up an unfinished upload: same session, and the files already sent are skipped.
      const keys = new Set(items.map(fileKey));
      const resuming = pending && pending.files.some((f) => keys.has(f.key)) ? pending.sessionId : null;
      const sid =
        resuming ??
        (await startSession(data.token, {
          name: name || undefined,
          email: email || undefined,
          message: message || undefined,
          project: project.trim() || undefined,
          password: password || undefined,
          code: code || undefined,
          total_bytes: totalBytes,
        }));
      await savePendingUpload(data.token, sid, items);
      pending = null;
      sessionId = sid;
      // Code accepted by the server — consume it so it never lingers in the UI.
      verified = true;
      code = "";
      // The server accepted these details, so they're worth remembering for next time.
      if (name.trim() || email.trim()) saveUploaderPrefs(name.trim(), email.trim());
      const result = await runUpload(data.token, sid, items, {
        onBytes: (d) => (uploadedBytes += d),
        onFile: (path: string, patch: FilePatch) => {
          const cur = fileViews[path] ?? { state: "queued", bytes: 0, retries: 0 };
          fileViews[path] = { ...cur, ...patch };
        },
      });
      receiptTo = result.receipt_to ?? null;
      deliveredAt = new Date();
      phase = "done";
    } catch (e) {
      // Tell the server this session is paused so it leaves "Uploading now" right away. Trying
      // again picks up where this stopped (same session, finished files skipped).
      if (sessionId) abandonSession(data.token, sessionId);
      pending = await loadPendingUpload(data.token);
      error = e instanceof Error ? e.message : "Upload failed.";
      phase = "error";
    }
  }

  // "Forgot something?" — back to a fresh form. Identity fields and verification
  // survive so the uploader can send another batch without re-doing the gate.
  function uploadMore() {
    items = [];
    fileViews = {};
    deliveredAt = null;
    receiptTo = null;
    uploadedBytes = 0;
    sessionId = null;
    error = null;
    phase = "select";
  }
</script>

<svelte:head><title>{link.display_name}</title></svelte:head>

<div class="flex min-h-screen flex-col items-center justify-center bg-bg px-4 py-12">
  <div class="w-full max-w-xl">
    <div class="mb-6 text-center">
      {#if link.logo_url}
        <img src={link.logo_url} alt="" class="mx-auto mb-4 h-12 object-contain" />
      {/if}
      <h1 class="text-2xl font-semibold tracking-tight">{link.display_name}</h1>
      {#if link.subtitle}<p class="mt-1 text-muted">{link.subtitle}</p>{/if}
    </div>

    {#if link.state !== "ok"}
      <Card class="text-center">
        <p class="text-muted">
          {link.state === "expired"
            ? "This upload link has expired."
            : "This upload link is no longer active."}
        </p>
      </Card>
    {:else if phase === "done"}
      <Card class="text-center">
        <div
          class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-success/15 text-success"
        >
          <svg class="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
            stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M20 6 9 17l-5-5" />
          </svg>
        </div>
        <p class="text-lg font-medium">Upload complete</p>
        <p class="mt-1 text-sm text-muted">
          {items.length} file{items.length === 1 ? "" : "s"} ({formatBytes(totalBytes)}) delivered.
        </p>
        <p class="mt-0.5 text-xs text-faint">{deliveredLabel}</p>
        <div class="mt-4 max-h-56 space-y-1 overflow-auto text-left text-sm">
          {#each items as it (it.path)}
            <div class="flex items-center justify-between rounded bg-success/10 px-2 py-1">
              <span class="min-w-0 [overflow-wrap:anywhere]">{it.path}</span>
              <span class="ml-2 flex shrink-0 items-center gap-2 text-xs text-faint">
                {formatBytes(it.file.size)}<span class="text-success">✓</span>
              </span>
            </div>
          {/each}
        </div>
        <CopyButton
          value={receipt}
          label="Copy list of files"
          title="Copy the list of delivered files"
          class="mt-2 text-xs"
        />
        {#if receiptTo}
          <p class="mt-1 text-xs text-faint">A copy is on its way to {receiptTo}.</p>
        {/if}
        <div class="mt-5">
          <Button {accent} onclick={uploadMore}>Forgot something? Send another</Button>
        </div>
      </Card>
    {:else}
      <Card class="space-y-5">
        {#if error}
          <p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">
            {error}{#if pending} Try again to pick up where it stopped.{/if}
          </p>
        {/if}
        {#if pending && phase === "select"}
          <div class="rounded-md border border-border bg-surface-2 px-3 py-2.5 text-sm">
            <p class="font-medium">An upload didn't finish</p>
            <p class="mt-0.5 text-muted">
              {pending.doneFiles} of {pending.files.length} file{pending.files.length === 1 ? "" : "s"} arrived ({formatBytes(pending.doneBytes)} of {formatBytes(pendingTotal)}).
              Add the same files again to send the rest.
            </p>
            <ul class="mt-1.5 space-y-0.5 text-xs text-faint">
              {#each pending.remaining.slice(0, 5) as path (path)}<li class="[overflow-wrap:anywhere]">{path}</li>{/each}
              {#if pending.remaining.length > 5}<li>and {pending.remaining.length - 5} more</li>{/if}
            </ul>
            <button type="button" class="mt-1 text-xs text-faint underline hover:text-text" onclick={discardPending}>Start over instead</button>
          </div>
        {/if}

        {#if link.uploader_fields_required.name || link.uploader_fields_required.email || link.uploader_fields_required.message || link.uploader_fields_required.project || link.password_required}
          <div class="space-y-3">
            {#if link.uploader_fields_required.name}
              <input bind:value={name} placeholder="Your name" disabled={phase === "uploading"} class={inputCls} />
            {/if}
            {#if link.uploader_fields_required.email}
              <div>
                <input bind:value={email} type="email" placeholder="Your email"
                  disabled={phase === "uploading" || codeSent} class="{inputCls} disabled:bg-surface-2" />
                {#if link.verify_email && verified}
                  <p class="mt-1 flex items-center gap-1.5 text-xs text-success">✓ Email verified</p>
                {/if}
              </div>
            {/if}
            {#if link.uploader_fields_required.project}
              <input bind:value={project} placeholder="Project" disabled={phase === "uploading"} class={inputCls} />
            {/if}
            {#if link.uploader_fields_required.message}
              <textarea bind:value={message} placeholder="Message" disabled={phase === "uploading"} class={inputCls}></textarea>
            {/if}
            {#if link.password_required}
              <input bind:value={password} type="password" placeholder="Password" disabled={phase === "uploading"} class={inputCls} />
            {/if}
          </div>
        {/if}

        {#if phase !== "uploading"}
          <div class="relative">
            <!-- OTP gate: covers the dropzone until the email is verified, then slides away. -->
            {#if locked}
              <div
                class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 rounded-lg border border-border bg-surface px-6 text-center"
                transition:fly={{ y: 18, duration: 320 }}
              >
                <div>
                  <p class="text-sm font-medium">Verify your email to upload</p>
                  <p class="mt-1 text-xs text-muted">
                    {#if codeSent}Enter the 6-digit code we sent to {email}.{:else}We'll email a one-time passcode to {email || "your email"}.{/if}
                  </p>
                </div>
                {#if codeSent}
                  <input
                    bind:value={code}
                    inputmode="numeric"
                    placeholder="······"
                    onkeydown={(e) => e.key === "Enter" && verifyAndUnlock()}
                    class="w-44 rounded-md border border-border px-3 py-2 text-center text-lg tracking-[0.4em] outline-none focus:border-accent"
                  />
                  <div class="flex items-center gap-3">
                    <Button {accent} onclick={verifyAndUnlock} disabled={verifying || !code.trim()}>
                      {verifying ? "Verifying…" : "Verify & continue"}
                    </Button>
                    <Button variant="subtle" size="sm" onclick={sendCode} disabled={sendingCode}>
                      {sendingCode ? "Sending…" : "Resend"}
                    </Button>
                  </div>
                {:else}
                  <Button {accent} onclick={sendCode} disabled={!email.trim() || sendingCode}>
                    {sendingCode ? "Sending…" : "Send verification code"}
                  </Button>
                {/if}
              </div>
            {/if}

            <!-- Drag-drop is a mouse enhancement; the buttons below are the keyboard-accessible
                 path (they open the hidden inputs), so the zone itself is a labelled group. -->
            <div
              role="group"
              aria-label="Upload files"
              ondragover={(e) => { if (!locked) { e.preventDefault(); dragging = true; } }}
              ondragleave={() => (dragging = false)}
              ondrop={(e) => { if (!locked) onDrop(e); }}
              class="rounded-card border-2 border-dashed p-8 text-center transition-all {dragging ? 'border-accent bg-surface-2' : 'border-border'} {locked ? 'pointer-events-none opacity-30 blur-[1px]' : ''}"
            >
              <p class="text-sm text-muted">Drag files or folders here</p>
              <p class="my-2 text-xs text-faint">or</p>
              <div class="flex justify-center gap-2">
                <Button variant="ghost" size="sm" onclick={() => filesInput?.click()}>Add files</Button>
                <Button variant="ghost" size="sm" onclick={() => folderInput?.click()}>Add folder</Button>
                <input bind:this={filesInput} type="file" multiple class="hidden" onchange={onPick} />
                <input bind:this={folderInput} type="file" webkitdirectory class="hidden" onchange={onPick} />
              </div>
              {#if link.allowed_extensions}
                <p class="mt-3 text-xs text-faint">Allowed: {link.allowed_extensions.join(", ")}</p>
              {/if}
            </div>
          </div>
        {/if}

        {#if items.length > 0}
          <div>
            <div class="max-h-72 divide-y divide-border overflow-auto rounded-md border border-border text-sm">
              {#each items as it (it.path)}
                {@const v = fileViews[it.path]}
                {@const pct = v && it.file.size ? Math.min(100, (v.bytes / it.file.size) * 100) : 0}
                <div class="px-3 py-2">
                  <div class="flex items-center gap-3">
                    <span class="min-w-0 flex-1 [overflow-wrap:anywhere]">{it.path}</span>
                    <span class="shrink-0 text-xs text-faint tabular-nums">{formatBytes(it.file.size)}</span>
                    {#if phase === "select"}
                      <button onclick={() => removeItem(it.path)} class="shrink-0 text-faint hover:text-danger" aria-label="Remove {it.path}">✕</button>
                    {:else if v}
                      <span class="w-28 shrink-0 text-right text-xs {v.state === 'done' ? 'text-success' : v.state === 'failed' ? 'text-danger' : v.state === 'retrying' ? 'text-warning' : 'text-muted'}">
                        {#if v.state === "done"}Delivered ✓
                        {:else if v.state === "verifying"}Verifying…
                        {:else if v.state === "retrying"}Retrying ({v.retries})
                        {:else if v.state === "uploading"}{pct.toFixed(0)}%
                        {:else if v.state === "failed"}Failed
                        {:else}Waiting{/if}
                      </span>
                    {/if}
                  </div>
                  {#if v && v.state !== "queued" && v.state !== "done"}
                    <div class="mt-1.5 h-1 overflow-hidden rounded-full bg-surface-3">
                      <div class="h-full rounded-full transition-[width] duration-300" style="width:{v.state === 'verifying' ? 100 : pct}%; background:{accent}"></div>
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          </div>
        {/if}

        {#if phase === "uploading"}
          <div>
            <div class="h-2 w-full overflow-hidden rounded-full bg-surface-3">
              <div class="h-full rounded-full transition-all" style="width:{percent}%; background:{accent}"></div>
            </div>
            <p class="mt-2 flex flex-wrap justify-center gap-x-2 text-center text-xs text-muted tabular-nums">
              <span>{formatBytes(uploadedBytes)} of {formatBytes(totalBytes)} ({percent.toFixed(0)}%)</span>
              <span>· {doneCount} of {items.length} file{items.length === 1 ? "" : "s"}</span>
              {#if speed > 0}<span>· {formatBytes(speed)}/s</span>{/if}
              {#if etaText}<span class="text-text">· {etaText}</span>{/if}
            </p>
            <p class="mt-1 text-center text-xs text-faint">You can close this tab; open the link again and add the same files to finish.</p>
          </div>
        {:else if !locked}
          <Button {accent} onclick={start} disabled={items.length === 0} class="w-full">
            Upload {items.length > 0 ? `${items.length} file${items.length === 1 ? "" : "s"} (${formatBytes(totalBytes)})` : ""}
          </Button>
        {/if}
        {#if phase !== "uploading" && link.expires_at}<p class="-mt-3 text-center text-xs text-faint">Expires on {expiryDate(link.expires_at)}</p>{/if}
      </Card>
    {/if}

    <PoweredByPortal />
  </div>
</div>
