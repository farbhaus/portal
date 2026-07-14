<script lang="ts">
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { Button, Card, PoweredByPortal } from "$lib/components";
  import { formatBytes } from "$lib/format";
  import {
    abandonSession,
    fileExtension,
    heartbeat,
    itemsFromDataTransfer,
    itemsFromFileList,
    requestCode,
    runUpload,
    startSession,
    verifyCode,
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
  let password = $state("");
  let code = $state("");
  let codeSent = $state(false);
  let verified = $state(false); // device already trusted; no code needed
  let sendingCode = $state(false);
  let verifying = $state(false);

  // The uploader is gated until the email is verified (for verify_email links).
  const locked = $derived(link.verify_email && !verified);

  // Prefill for returning uploaders. Never touches `verified` — a verify_email
  // link still gates on the OTP / device-trust cookie exactly as for a blank form.
  onMount(() => {
    const prefs = loadUploaderPrefs();
    if (!prefs) return;
    if (!name) name = prefs.name;
    if (!email) email = prefs.email;
  });

  let items = $state<UploadItem[]>([]);
  let dragging = $state(false);
  let filesInput = $state<HTMLInputElement>();
  let folderInput = $state<HTMLInputElement>();
  let phase = $state<"select" | "uploading" | "done" | "error">("select");
  let error = $state<string | null>(null);
  let sessionId = $state<string | null>(null);
  let uploadedBytes = $state(0);
  let currentFile = $state<string | null>(null);
  let doneFiles = $state<Set<string>>(new Set());

  const totalBytes = $derived(items.reduce((s, it) => s + it.file.size, 0));
  const percent = $derived(totalBytes > 0 ? Math.min(100, (uploadedBytes / totalBytes) * 100) : 0);

  // Liveness for the server while uploading: a progress heartbeat, plus a best-effort abandon
  // beacon if the tab goes away mid-upload — so the admin dashboard never shows ghost uploads.
  // Reading uploadedBytes inside the interval callback is untracked, so the effect only re-runs
  // on phase/session changes, and its cleanup drops the pagehide listener once the upload ends.
  $effect(() => {
    if (phase !== "uploading" || !sessionId) return;
    const sid = sessionId;
    const t = setInterval(() => heartbeat(data.token, sid, uploadedBytes), 20_000);
    const onPageHide = () => abandonSession(data.token, sid);
    window.addEventListener("pagehide", onPageHide);
    return () => {
      clearInterval(t);
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
    doneFiles = new Set();
    try {
      const sid = await startSession(data.token, {
        name: name || undefined,
        email: email || undefined,
        message: message || undefined,
        password: password || undefined,
        code: code || undefined,
      });
      sessionId = sid;
      // Code accepted by the server — consume it so it never lingers in the UI.
      verified = true;
      code = "";
      // The server accepted these details, so they're worth remembering for next time.
      if (name.trim() || email.trim()) saveUploaderPrefs(name.trim(), email.trim());
      await runUpload(data.token, sid, items, {
        onBytes: (d) => (uploadedBytes += d),
        onFileStart: (p) => (currentFile = p),
        onFileDone: (p) => {
          doneFiles = new Set(doneFiles).add(p);
          currentFile = null;
        },
      });
      phase = "done";
    } catch (e) {
      // Tell the server this session is dead so it leaves "Uploading now" right away
      // (a retry starts a fresh session).
      if (sessionId) abandonSession(data.token, sessionId);
      error = e instanceof Error ? e.message : "Upload failed.";
      phase = "error";
    }
  }

  // "Forgot something?" — back to a fresh form. Identity fields and verification
  // survive so the uploader can send another batch without re-doing the gate.
  function uploadMore() {
    items = [];
    doneFiles = new Set();
    uploadedBytes = 0;
    sessionId = null;
    currentFile = null;
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
        <div class="mt-5">
          <Button {accent} onclick={uploadMore}>Forgot something? Send another</Button>
        </div>
      </Card>
    {:else}
      <Card class="space-y-5">
        {#if error}
          <p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
        {/if}

        {#if link.uploader_fields_required.name || link.uploader_fields_required.email || link.uploader_fields_required.message || link.password_required}
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
            <div class="max-h-56 space-y-1 overflow-auto text-sm">
              {#each items as it (it.path)}
                <div class="flex items-center justify-between rounded px-2 py-1 {doneFiles.has(it.path) ? 'bg-success/10' : currentFile === it.path ? 'bg-surface-2' : ''}">
                  <span class="truncate">{it.path}</span>
                  <span class="ml-2 flex shrink-0 items-center gap-2 text-xs text-faint">
                    {formatBytes(it.file.size)}
                    {#if doneFiles.has(it.path)}<span class="text-success">✓</span>
                    {:else if currentFile === it.path}<span class="text-muted">…</span>
                    {:else if phase === "select"}
                      <button onclick={() => removeItem(it.path)} class="text-faint hover:text-danger" aria-label="Remove {it.path}">✕</button>
                    {/if}
                  </span>
                </div>
              {/each}
            </div>
            {#if phase === "select"}
              <p class="mt-2 text-center text-xs text-faint">
                Add more files or folders above — everything uploads together.
              </p>
            {/if}
          </div>
        {/if}

        {#if phase === "uploading"}
          <div>
            <div class="h-2 w-full overflow-hidden rounded-full bg-surface-3">
              <div class="h-full rounded-full transition-all" style="width:{percent}%; background:{accent}"></div>
            </div>
            <p class="mt-2 text-center text-xs text-muted">
              {formatBytes(uploadedBytes)} / {formatBytes(totalBytes)} ({percent.toFixed(0)}%)
            </p>
          </div>
        {:else if !locked}
          <Button {accent} onclick={start} disabled={items.length === 0} class="w-full">
            Upload {items.length > 0 ? `${items.length} file${items.length === 1 ? "" : "s"} (${formatBytes(totalBytes)})` : ""}
          </Button>
        {/if}
      </Card>
    {/if}

    <PoweredByPortal />
  </div>
</div>
