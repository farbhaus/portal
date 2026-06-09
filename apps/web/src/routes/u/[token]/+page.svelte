<script lang="ts">
  import {
    fileExtension,
    formatBytes,
    itemsFromDataTransfer,
    itemsFromFileList,
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

  async function start() {
    const fieldErr = missingFields();
    if (fieldErr) {
      error = fieldErr;
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
      });
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

<div class="flex min-h-screen flex-col items-center bg-neutral-50 px-4 py-12">
  <div class="w-full max-w-xl">
    <div class="mb-6 text-center">
      {#if link.logo_url}
        <img src={link.logo_url} alt="" class="mx-auto mb-4 h-12 object-contain" />
      {/if}
      <h1 class="text-2xl font-semibold">{link.display_name}</h1>
      {#if link.subtitle}<p class="mt-1 text-neutral-500">{link.subtitle}</p>{/if}
    </div>

    {#if link.state !== "ok"}
      <div class="rounded-xl border border-neutral-200 bg-white p-8 text-center">
        <p class="text-neutral-600">
          {link.state === "expired"
            ? "This upload link has expired."
            : "This upload link is no longer active."}
        </p>
      </div>
    {:else if phase === "done"}
      <div class="rounded-xl border border-green-200 bg-white p-8 text-center">
        <p class="text-lg font-medium text-green-700">Upload complete 🎉</p>
        <p class="mt-1 text-sm text-neutral-500">
          {items.length} file{items.length === 1 ? "" : "s"} ({formatBytes(totalBytes)}) delivered.
        </p>
      </div>
    {:else}
      <div class="space-y-5 rounded-xl border border-neutral-200 bg-white p-6">
        {#if error}
          <p class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">{error}</p>
        {/if}

        {#if link.uploader_fields_required.name || link.uploader_fields_required.email || link.uploader_fields_required.message || link.password_required}
          <div class="space-y-3">
            {#if link.uploader_fields_required.name}
              <input bind:value={name} placeholder="Your name" disabled={phase === "uploading"}
                class="w-full rounded-md border border-neutral-300 px-3 py-2 text-sm" />
            {/if}
            {#if link.uploader_fields_required.email}
              <input bind:value={email} type="email" placeholder="Your email" disabled={phase === "uploading"}
                class="w-full rounded-md border border-neutral-300 px-3 py-2 text-sm" />
            {/if}
            {#if link.uploader_fields_required.message}
              <textarea bind:value={message} placeholder="Message" disabled={phase === "uploading"}
                class="w-full rounded-md border border-neutral-300 px-3 py-2 text-sm"></textarea>
            {/if}
            {#if link.password_required}
              <input bind:value={password} type="password" placeholder="Password" disabled={phase === "uploading"}
                class="w-full rounded-md border border-neutral-300 px-3 py-2 text-sm" />
            {/if}
          </div>
        {/if}

        {#if phase !== "uploading"}
          <div
            role="button" tabindex="0"
            ondragover={(e) => { e.preventDefault(); dragging = true; }}
            ondragleave={() => (dragging = false)}
            ondrop={onDrop}
            class="rounded-lg border-2 border-dashed p-8 text-center transition-colors {dragging ? 'border-neutral-900 bg-neutral-50' : 'border-neutral-300'}"
          >
            <p class="text-sm text-neutral-500">Drag files or folders here</p>
            <p class="my-2 text-xs text-neutral-400">or</p>
            <div class="flex justify-center gap-2">
              <label class="cursor-pointer rounded-md border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100">
                Choose files
                <input type="file" multiple class="hidden" onchange={onPick} />
              </label>
              <label class="cursor-pointer rounded-md border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100">
                Choose folder
                <input type="file" webkitdirectory class="hidden" onchange={onPick} />
              </label>
            </div>
            {#if link.allowed_extensions}
              <p class="mt-3 text-xs text-neutral-400">Allowed: {link.allowed_extensions.join(", ")}</p>
            {/if}
          </div>
        {/if}

        {#if items.length > 0}
          <div class="max-h-56 space-y-1 overflow-auto text-sm">
            {#each items as it (it.path)}
              <div class="flex items-center justify-between rounded px-2 py-1 {doneFiles.has(it.path) ? 'bg-green-50' : currentFile === it.path ? 'bg-neutral-100' : ''}">
                <span class="truncate">{it.path}</span>
                <span class="ml-2 flex shrink-0 items-center gap-2 text-xs text-neutral-400">
                  {formatBytes(it.file.size)}
                  {#if doneFiles.has(it.path)}<span class="text-green-600">✓</span>
                  {:else if currentFile === it.path}<span class="text-neutral-500">…</span>
                  {:else if phase === "select"}
                    <button onclick={() => removeItem(it.path)} class="text-neutral-400 hover:text-red-600">✕</button>
                  {/if}
                </span>
              </div>
            {/each}
          </div>
        {/if}

        {#if phase === "uploading"}
          <div>
            <div class="h-2 w-full overflow-hidden rounded-full bg-neutral-200">
              <div class="h-full rounded-full transition-all" style="width:{percent}%; background:{accent}"></div>
            </div>
            <p class="mt-2 text-center text-xs text-neutral-500">
              {formatBytes(uploadedBytes)} / {formatBytes(totalBytes)} ({percent.toFixed(0)}%)
            </p>
          </div>
        {:else}
          <button
            onclick={start}
            disabled={items.length === 0}
            class="w-full rounded-md px-4 py-2.5 text-sm font-medium text-white disabled:opacity-50"
            style="background:{accent}"
          >
            Upload {items.length > 0 ? `${items.length} file${items.length === 1 ? "" : "s"} (${formatBytes(totalBytes)})` : ""}
          </button>
        {/if}
      </div>
    {/if}

    <p class="mt-6 text-center text-xs text-neutral-400">Powered by Portal</p>
  </div>
</div>
