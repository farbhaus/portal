<script lang="ts">
  import {
    fileExtension,
    formatBytes,
    itemsFromDataTransfer,
    itemsFromFileList,
    requestCode,
    runUpload,
    startSession,
    type UploadItem,
  } from "$lib/upload";

  let { data } = $props();
  const link = $derived(data.link);
  const accent = $derived(link.accent_color || "#111111");

  let name = $state("");
  let email = $state("");
  let message = $state("");
  let password = $state("");
  let code = $state("");
  let codeSent = $state(false);
  let verified = $state(false); // device already trusted; no code needed
  let sendingCode = $state(false);

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
              <input bind:value={email} type="email" placeholder="Your email" disabled={phase === "uploading" || codeSent}
                class="w-full rounded-md border border-border px-3 py-2 text-sm disabled:bg-surface-2" />
            {/if}
            {#if link.uploader_fields_required.message}
              <textarea bind:value={message} placeholder="Message" disabled={phase === "uploading"}
                class="w-full rounded-md border border-border px-3 py-2 text-sm"></textarea>
            {/if}
            {#if link.password_required}
              <input bind:value={password} type="password" placeholder="Password" disabled={phase === "uploading"}
                class="w-full rounded-md border border-border px-3 py-2 text-sm" />
            {/if}
            {#if link.verify_email && verified}
              <p class="flex items-center gap-1.5 text-xs text-success">✓ Email verified</p>
            {:else if link.verify_email && codeSent}
              <div>
                <input bind:value={code} inputmode="numeric" placeholder="Enter the 6-digit code"
                  disabled={phase === "uploading"}
                  class="w-full rounded-md border border-border px-3 py-2 text-sm tracking-widest" />
                <p class="mt-1 text-xs text-muted">We emailed a code to {email}.
                  <button type="button" onclick={sendCode} class="underline hover:text-text">Resend</button></p>
              </div>
            {/if}
          </div>
        {/if}

        {#if phase !== "uploading"}
          <div
            role="button" tabindex="0"
            ondragover={(e) => { e.preventDefault(); dragging = true; }}
            ondragleave={() => (dragging = false)}
            ondrop={onDrop}
            class="rounded-lg border-2 border-dashed p-8 text-center transition-colors {dragging ? 'border-accent bg-surface-2' : 'border-border'}"
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
        {:else if link.verify_email && !verified && !codeSent}
          <button
            onclick={sendCode}
            disabled={!email.trim() || sendingCode}
            class="w-full rounded-md px-4 py-2.5 text-sm font-medium text-on-accent disabled:opacity-50"
            style="background:{accent}"
          >
            {sendingCode ? "Sending…" : "Send verification code"}
          </button>
        {:else}
          <button
            onclick={start}
            disabled={items.length === 0 || (link.verify_email && !verified && !code.trim())}
            class="w-full rounded-md px-4 py-2.5 text-sm font-medium text-on-accent disabled:opacity-50"
            style="background:{accent}"
          >
            Upload {items.length > 0 ? `${items.length} file${items.length === 1 ? "" : "s"} (${formatBytes(totalBytes)})` : ""}
          </button>
        {/if}
      </div>
    {/if}

    <p class="mt-6 text-center text-xs text-faint">Powered by Portal</p>
  </div>
</div>
