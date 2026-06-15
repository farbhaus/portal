<script lang="ts">
  import { fly } from "svelte/transition";
  import {
    fileExtension,
    formatBytes,
    itemsFromDataTransfer,
    itemsFromFileList,
    requestCode,
    runUpload,
    startSession,
    verifyCode,
    type UploadItem,
  } from "$lib/upload";

  let { data } = $props();
  const link = $derived(data.link);
  const accent = $derived(link.accent_color || "#f59e0b");
  function accentText(hex: string): string {
    const r = parseInt(hex.slice(1, 3), 16);
    const g = parseInt(hex.slice(3, 5), 16);
    const b = parseInt(hex.slice(5, 7), 16);
    return (0.299 * r + 0.587 * g + 0.114 * b) / 255 > 0.5 ? "#1a1206" : "#ffffff";
  }
  const onAccent = $derived(accentText(accent));

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

  let items = $state<UploadItem[]>([]);
  let dragging = $state(false);
  let phase = $state<"select" | "uploading" | "done" | "error">("select");
  let error = $state<string | null>(null);
  let uploadedBytes = $state(0);
  let currentFile = $state<string | null>(null);
  let doneFiles = $state<Set<string>>(new Set());

  const totalBytes = $derived(items.reduce((s, it) => s + it.file.size, 0));
  const percent = $derived(totalBytes > 0 ? Math.min(100, (uploadedBytes / totalBytes) * 100) : 0);

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
    uploadedBytes = 0;
    doneFiles = new Set();
    try {
      const sessionId = await startSession(data.token, {
        name: name || undefined,
        email: email || undefined,
        message: message || undefined,
        password: password || undefined,
        code: code || undefined,
      });
      // Code accepted by the server — consume it so it never lingers in the UI.
      verified = true;
      code = "";
      await runUpload(data.token, sessionId, items, {
        onBytes: (d) => (uploadedBytes += d),
        onFileStart: (p) => (currentFile = p),
        onFileDone: (p) => {
          doneFiles = new Set(doneFiles).add(p);
          currentFile = null;
        },
      });
      phase = "done";
    } catch (e) {
      error = e instanceof Error ? e.message : "Upload failed.";
      phase = "error";
    }
  }
</script>

<svelte:head><title>{link.display_name}</title></svelte:head>

<div class="flex min-h-screen flex-col items-center bg-surface-2 px-4 py-12">
  <div class="w-full max-w-xl">
    <div class="mb-6 text-center">
      {#if link.logo_url}
        <img src={link.logo_url} alt="" class="mx-auto mb-4 h-12 object-contain" />
      {/if}
      <h1 class="text-2xl font-semibold">{link.display_name}</h1>
      {#if link.subtitle}<p class="mt-1 text-muted">{link.subtitle}</p>{/if}
    </div>

    {#if link.state !== "ok"}
      <div class="rounded-xl border border-border bg-surface p-8 text-center">
        <p class="text-muted">
          {link.state === "expired"
            ? "This upload link has expired."
            : "This upload link is no longer active."}
        </p>
      </div>
    {:else if phase === "done"}
      <div class="rounded-xl border border-success/30 bg-surface p-8 text-center">
        <p class="text-lg font-medium text-success">Upload complete 🎉</p>
        <p class="mt-1 text-sm text-muted">
          {items.length} file{items.length === 1 ? "" : "s"} ({formatBytes(totalBytes)}) delivered.
        </p>
      </div>
    {:else}
      <div class="space-y-5 rounded-xl border border-border bg-surface p-6">
        {#if error}
          <p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
        {/if}

        {#if link.uploader_fields_required.name || link.uploader_fields_required.email || link.uploader_fields_required.message || link.password_required}
          <div class="space-y-3">
            {#if link.uploader_fields_required.name}
              <input bind:value={name} placeholder="Your name" disabled={phase === "uploading"}
                class="w-full rounded-md border border-border px-3 py-2 text-sm" />
            {/if}
            {#if link.uploader_fields_required.email}
              <div>
                <input bind:value={email} type="email" placeholder="Your email" disabled={phase === "uploading" || codeSent}
                  class="w-full rounded-md border border-border px-3 py-2 text-sm disabled:bg-surface-2" />
                {#if link.verify_email && verified}
                  <p class="mt-1 flex items-center gap-1.5 text-xs text-success">✓ Email verified</p>
                {/if}
              </div>
            {/if}
            {#if link.uploader_fields_required.message}
              <textarea bind:value={message} placeholder="Message" disabled={phase === "uploading"}
                class="w-full rounded-md border border-border px-3 py-2 text-sm"></textarea>
            {/if}
            {#if link.password_required}
              <input bind:value={password} type="password" placeholder="Password" disabled={phase === "uploading"}
                class="w-full rounded-md border border-border px-3 py-2 text-sm" />
            {/if}
          </div>
        {/if}

        {#if phase !== "uploading"}
          <div class="relative">
            <!-- OTP gate: covers the dropzone until the email is verified, then slides away. -->
            {#if locked}
              <div
                class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 rounded-lg border border-accent/40 bg-surface px-6 text-center"
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
                    class="w-44 rounded-md border border-border px-3 py-2 text-center text-lg tracking-[0.4em]"
                  />
                  <div class="flex items-center gap-3">
                    <button onclick={verifyAndUnlock} disabled={verifying || !code.trim()}
                      class="rounded-md px-4 py-2 text-sm font-medium disabled:opacity-50" style="background:{accent};color:{onAccent}">
                      {verifying ? "Verifying…" : "Verify & continue"}
                    </button>
                    <button type="button" onclick={sendCode} disabled={sendingCode} class="text-xs text-muted underline hover:text-text disabled:opacity-50">
                      {sendingCode ? "Sending…" : "Resend"}
                    </button>
                  </div>
                {:else}
                  <button onclick={sendCode} disabled={!email.trim() || sendingCode}
                    class="rounded-md px-4 py-2 text-sm font-medium disabled:opacity-50" style="background:{accent};color:{onAccent}">
                    {sendingCode ? "Sending…" : "Send verification code"}
                  </button>
                {/if}
              </div>
            {/if}

            <div
              role="button" tabindex="0"
              ondragover={(e) => { if (!locked) { e.preventDefault(); dragging = true; } }}
              ondragleave={() => (dragging = false)}
              ondrop={(e) => { if (!locked) onDrop(e); }}
              class="rounded-lg border-2 border-dashed p-8 text-center transition-all {dragging ? 'border-accent bg-surface-2' : 'border-border'} {locked ? 'pointer-events-none opacity-30 blur-[1px]' : ''}"
            >
              <p class="text-sm text-muted">Drag files or folders here</p>
              <p class="my-2 text-xs text-faint">or</p>
              <div class="flex justify-center gap-2">
                <label class="cursor-pointer rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-2">
                  Choose files
                  <input type="file" multiple class="hidden" onchange={onPick} />
                </label>
                <label class="cursor-pointer rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-2">
                  Choose folder
                  <input type="file" webkitdirectory class="hidden" onchange={onPick} />
                </label>
              </div>
              {#if link.allowed_extensions}
                <p class="mt-3 text-xs text-faint">Allowed: {link.allowed_extensions.join(", ")}</p>
              {/if}
            </div>
          </div>
        {/if}

        {#if items.length > 0}
          <div class="max-h-56 space-y-1 overflow-auto text-sm">
            {#each items as it (it.path)}
              <div class="flex items-center justify-between rounded px-2 py-1 {doneFiles.has(it.path) ? 'bg-success/10' : currentFile === it.path ? 'bg-surface-2' : ''}">
                <span class="truncate">{it.path}</span>
                <span class="ml-2 flex shrink-0 items-center gap-2 text-xs text-faint">
                  {formatBytes(it.file.size)}
                  {#if doneFiles.has(it.path)}<span class="text-success">✓</span>
                  {:else if currentFile === it.path}<span class="text-muted">…</span>
                  {:else if phase === "select"}
                    <button onclick={() => removeItem(it.path)} class="text-faint hover:text-danger">✕</button>
                  {/if}
                </span>
              </div>
            {/each}
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
          <button
            onclick={start}
            disabled={items.length === 0}
            class="w-full rounded-md px-4 py-2.5 text-sm font-medium disabled:opacity-50"
            style="background:{accent};color:{onAccent}"
          >
            Upload {items.length > 0 ? `${items.length} file${items.length === 1 ? "" : "s"} (${formatBytes(totalBytes)})` : ""}
          </button>
        {/if}
      </div>
    {/if}

    <p class="mt-6 text-center text-xs text-faint">Powered by Portal</p>
  </div>
</div>
