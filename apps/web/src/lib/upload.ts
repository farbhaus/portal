import { kvDel, kvGet, kvKeys, kvSet } from "$lib/resume-store";

// Browser-side chunked uploader. Bytes go directly from the browser to Frame.io's presigned
// S3 URLs — never through Portal. The Portal API only issues the URLs and tracks state.

export type ChunkSpec = { part: number; size: number; url: string };
export type FileCreateResult = {
  file_id: string;
  chunks: ChunkSpec[];
  headers: Record<string, string>;
};

export type UploadItem = { file: File; path: string };

const CHUNK_CONCURRENCY = 4;
const MAX_RETRIES = 5;

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function asJson(res: Response): Promise<any> {
  return res.json().catch(() => ({}));
}

export async function startSession(
  token: string,
  fields: {
    name?: string;
    email?: string;
    message?: string;
    project?: string;
    password?: string;
    code?: string;
    // Size of the whole batch, so the dashboard's "Uploading now" shows the real total from the
    // start instead of one that grows as each file is registered.
    total_bytes?: number;
  },
): Promise<string> {
  const res = await fetch(`/api/public/links/${token}/sessions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(fields),
  });
  if (!res.ok) throw new Error((await asJson(res)).detail ?? "Could not start the upload");
  return (await asJson(res)).session_id as string;
}

// Ask Portal to email a verification code (or report this device is already trusted).
export async function requestCode(
  token: string,
  email: string,
): Promise<{ trusted: boolean; sent: boolean }> {
  const res = await fetch(`/api/public/links/${token}/request-code`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email }),
  });
  if (!res.ok) throw new Error((await asJson(res)).detail ?? "Could not send a code");
  return asJson(res);
}

// Validate the OTP up front; on success the device is trusted and the uploader unlocks.
export async function verifyCode(token: string, email: string, code: string): Promise<void> {
  const res = await fetch(`/api/public/links/${token}/verify-code`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, code }),
  });
  if (!res.ok) throw new Error((await asJson(res)).detail ?? "That code is incorrect or expired");
}

// receipt_to: where the server is emailing the uploader's delivery receipt (verified links only).
export type CompleteResult = { ok: boolean; receipt_to?: string | null };

export async function completeSession(
  token: string,
  sessionId: string,
  totalBytes: number,
): Promise<CompleteResult> {
  // Every file is already delivered by now, so this call failing — even on a dropped connection —
  // mustn't send the uploader to the error screen and a needless re-upload. Retry briefly (the
  // server notifies only on the first completion, so a retry can't double-mail), then let it go.
  for (let attempt = 0; attempt < 3; attempt++) {
    if (attempt > 0) await sleep(attempt * 2000);
    try {
      const res = await fetch(`/api/public/links/${token}/sessions/${sessionId}/complete`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ total_bytes: totalBytes }),
      });
      if (res.ok) return asJson(res);
      if (res.status < 500) break; // a client error won't improve on retry
    } catch {
      // network blip — try again
    }
  }
  return { ok: false };
}

// Fire-and-forget liveness ping while an upload runs (carries progress for the admin dashboard).
// Failures are harmless: the next beat revives a session the server sweep flagged too eagerly.
export async function heartbeat(
  token: string,
  sessionId: string,
  uploadedBytes: number,
): Promise<void> {
  try {
    await fetch(`/api/public/links/${token}/sessions/${sessionId}/heartbeat`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ uploaded_bytes: uploadedBytes }),
    });
  } catch {
    // Next beat (or the server sweep) covers it.
  }
}

// Best-effort "tab is going away" signal so the dashboard clears immediately; sendBeacon
// survives page teardown (and the server sweep catches the cases where neither arrives).
export function abandonSession(token: string, sessionId: string): void {
  const url = `/api/public/links/${token}/sessions/${sessionId}/abandon`;
  if (navigator.sendBeacon?.(url)) return;
  fetch(url, { method: "POST", keepalive: true }).catch(() => {});
}

// ── per-file state, shared with the page ──────────────────────────────────────

export type FileState = "queued" | "uploading" | "retrying" | "verifying" | "done" | "failed";
export type FilePatch = { state?: FileState; bytes?: number; retries?: number; error?: string };

export type ProgressHandlers = {
  // Aggregate byte delta across all files (negative to roll back a failed attempt).
  onBytes: (delta: number) => void;
  // A file's state, bytes so far, or retry count changed.
  onFile: (path: string, patch: FilePatch) => void;
};

// A chunk URL Frame.io no longer accepts (presigned URLs expire); the file has to start over.
class ExpiredUrlError extends Error {}

// PUT one chunk via XHR so we get upload progress events. onDelta receives byte increments
// (and negative deltas to roll back a failed attempt so the aggregate stays accurate).
function putChunk(
  url: string,
  blob: Blob,
  headers: Record<string, string>,
  onDelta: (bytes: number) => void,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open("PUT", url, true);
    for (const [k, v] of Object.entries(headers)) xhr.setRequestHeader(k, v);
    let last = 0;
    xhr.upload.onprogress = (e) => {
      onDelta(e.loaded - last);
      last = e.loaded;
    };
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        onDelta(blob.size - last);
        resolve();
      } else if (xhr.status === 403) reject(new ExpiredUrlError("Upload URL expired"));
      else reject(new Error(`Upload failed (${xhr.status})`));
    };
    xhr.onerror = () => reject(new Error("Network error during upload"));
    xhr.send(blob);
  });
}

async function uploadChunkWithRetry(
  chunk: ChunkSpec,
  blob: Blob,
  headers: Record<string, string>,
  onBytes: (delta: number) => void,
  onRetry: () => void,
): Promise<void> {
  for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
    let counted = 0;
    try {
      await putChunk(chunk.url, blob, headers, (d) => {
        counted += d;
        onBytes(d);
      });
      return;
    } catch (err) {
      onBytes(-counted); // roll back this attempt's progress
      if (err instanceof ExpiredUrlError || attempt === MAX_RETRIES - 1) throw err;
      onRetry();
      await sleep(1000 * 2 ** attempt); // exponential backoff (resume after a disconnect)
    }
  }
}

// After the first failure no runner takes new work, so a file that starts over (expired URLs)
// isn't raced by the old attempt's leftover chunks.
async function pool<T>(items: T[], limit: number, worker: (item: T) => Promise<void>): Promise<void> {
  let i = 0;
  let failed = false;
  const runners = Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (!failed && i < items.length) {
      const idx = i++;
      try {
        await worker(items[idx]);
      } catch (e) {
        failed = true;
        throw e;
      }
    }
  });
  await Promise.all(runners);
}

async function pollComplete(token: string, sessionId: string, fileId: string): Promise<void> {
  for (let attempt = 0; attempt < 60; attempt++) {
    const res = await fetch(
      `/api/public/links/${token}/sessions/${sessionId}/files/${fileId}/complete`,
      { method: "POST" },
    );
    const body = await asJson(res);
    if (body.failed) throw new Error("Frame.io reported the upload failed");
    if (body.complete) return;
    await sleep(2000);
  }
  // Assembly is taking unusually long; treat as done rather than blocking the uploader forever.
}

// ── resume after a reload ───────────────────────────────────────────────────────
// What a page needs to pick an interrupted upload back up: the session, and per file the chunk
// URLs Frame.io issued plus which parts already landed. Browsers can't reopen files after a
// reload, so the uploader drops the same files again; they're matched by path, size and date.

type FileRecord = { fileId: string; chunks: ChunkSpec[]; headers: Record<string, string> };
type Progress = { done: number[]; complete: boolean };
export type PendingUpload = {
  sessionId: string;
  startedAt: string;
  files: { key: string; path: string; size: number }[];
};

export const fileKey = (it: UploadItem) => `${it.path}|${it.file.size}|${it.file.lastModified}`;

export type PendingSummary = PendingUpload & { doneBytes: number; doneFiles: number; remaining: string[] };

export async function loadPendingUpload(token: string): Promise<PendingSummary | null> {
  const p = await kvGet<PendingUpload>(`sess:${token}`);
  if (!p) return null;
  let doneBytes = 0;
  let doneFiles = 0;
  const remaining: string[] = [];
  for (const f of p.files) {
    const prog = await kvGet<Progress>(`prog:${token}:${f.key}`);
    if (prog?.complete) {
      doneFiles++;
      doneBytes += f.size;
      continue;
    }
    remaining.push(f.path);
    const rec = await kvGet<FileRecord>(`file:${token}:${f.key}`);
    if (rec && prog) doneBytes += rec.chunks.filter((c) => prog.done.includes(c.part)).reduce((n, c) => n + c.size, 0);
  }
  return { ...p, doneBytes, doneFiles, remaining };
}

export async function savePendingUpload(token: string, sessionId: string, items: UploadItem[]) {
  const p: PendingUpload = {
    sessionId,
    startedAt: new Date().toISOString(),
    files: items.map((it) => ({ key: fileKey(it), path: it.path, size: it.file.size })),
  };
  await kvSet(`sess:${token}`, p);
}

export async function clearPendingUpload(token: string) {
  await kvDel(`sess:${token}`);
  for (const k of [...(await kvKeys(`file:${token}:`)), ...(await kvKeys(`prog:${token}:`))]) await kvDel(k);
}

// Presigned S3 URLs carry their own lifetime (X-Amz-Date + X-Amz-Expires).
function urlsExpired(chunks: ChunkSpec[]): boolean {
  try {
    const u = new URL(chunks[0].url);
    const date = u.searchParams.get("X-Amz-Date");
    const ttl = Number(u.searchParams.get("X-Amz-Expires"));
    if (!date || !ttl) return false;
    const t = Date.UTC(+date.slice(0, 4), +date.slice(4, 6) - 1, +date.slice(6, 8), +date.slice(9, 11), +date.slice(11, 13), +date.slice(13, 15));
    return Date.now() > t + ttl * 1000 - 5 * 60_000;
  } catch {
    return false;
  }
}

async function uploadOne(
  token: string,
  sessionId: string,
  item: UploadItem,
  handlers: ProgressHandlers,
  fresh = false,
): Promise<void> {
  const key = fileKey(item);
  const { onBytes, onFile } = handlers;
  let fileBytes = 0;
  const credit = (d: number) => {
    fileBytes += d;
    onBytes(d);
    onFile(item.path, { bytes: fileBytes });
  };

  let prog = (!fresh && (await kvGet<Progress>(`prog:${token}:${key}`))) || { done: [], complete: false };
  if (prog.complete) {
    credit(item.file.size);
    onFile(item.path, { state: "done" });
    return;
  }
  let rec = fresh ? undefined : await kvGet<FileRecord>(`file:${token}:${key}`);
  if (rec && urlsExpired(rec.chunks)) rec = undefined;
  if (!rec) {
    const res = await fetch(`/api/public/links/${token}/sessions/${sessionId}/files`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: item.path, size: item.file.size }),
    });
    if (!res.ok) throw new Error((await asJson(res)).detail ?? `Could not upload ${item.path}`);
    const result = (await asJson(res)) as FileCreateResult;
    rec = { fileId: result.file_id, chunks: result.chunks, headers: result.headers };
    prog = { done: [], complete: false };
    await kvSet(`file:${token}:${key}`, rec);
    await kvSet(`prog:${token}:${key}`, prog);
  }

  // Parts that landed before the reload count straight away.
  let offset = 0;
  const jobs: { chunk: ChunkSpec; blob: Blob }[] = [];
  for (const chunk of rec.chunks) {
    if (prog.done.includes(chunk.part)) credit(chunk.size);
    else jobs.push({ chunk, blob: item.file.slice(offset, offset + chunk.size) });
    offset += chunk.size;
  }
  onFile(item.path, { state: "uploading" });
  let retries = 0;
  const record = rec;
  try {
    await pool(jobs, CHUNK_CONCURRENCY, async ({ chunk, blob }) => {
      await uploadChunkWithRetry(chunk, blob, record.headers, credit, () => {
        retries++;
        onFile(item.path, { state: "retrying", retries });
      });
      onFile(item.path, { state: "uploading" });
      prog.done.push(chunk.part);
      await kvSet(`prog:${token}:${key}`, prog);
    });
  } catch (e) {
    if (e instanceof ExpiredUrlError && !fresh) {
      // The URLs from before the reload ran out: this file starts over with new ones.
      credit(-fileBytes);
      return uploadOne(token, sessionId, item, handlers, true);
    }
    throw e;
  }
  onFile(item.path, { state: "verifying" });
  await pollComplete(token, sessionId, record.fileId);
  prog.complete = true;
  await kvSet(`prog:${token}:${key}`, prog);
  onFile(item.path, { state: "done" });
}

// Upload files one at a time (chunks within a file go in parallel) so a huge drop doesn't open
// hundreds of sockets. Returns when every file is uploaded and the session is completed.
export async function runUpload(
  token: string,
  sessionId: string,
  items: UploadItem[],
  handlers: ProgressHandlers,
): Promise<CompleteResult> {
  for (const item of items) {
    try {
      await uploadOne(token, sessionId, item, handlers);
    } catch (e) {
      handlers.onFile(item.path, { state: "failed", error: e instanceof Error ? e.message : "Upload failed" });
      throw e;
    }
  }
  const total = items.reduce((sum, it) => sum + it.file.size, 0);
  const result = await completeSession(token, sessionId, total);
  await clearPendingUpload(token);
  return result;
}

// ── building the file list (preserving folder structure) ──────────────────────

async function readAllEntries(reader: any): Promise<any[]> {
  const out: any[] = [];
  while (true) {
    const batch: any[] = await new Promise((res, rej) => reader.readEntries(res, rej));
    if (batch.length === 0) break;
    out.push(...batch);
  }
  return out;
}

async function walkEntry(entry: any, prefix: string, out: UploadItem[]): Promise<void> {
  if (entry.isFile) {
    const file: File = await new Promise((res, rej) => entry.file(res, rej));
    out.push({ file, path: prefix + entry.name });
  } else if (entry.isDirectory) {
    const entries = await readAllEntries(entry.createReader());
    for (const child of entries) await walkEntry(child, prefix + entry.name + "/", out);
  }
}

// Drag-drop: traverse dropped folders to keep their structure.
export async function itemsFromDataTransfer(dt: DataTransfer): Promise<UploadItem[]> {
  const entries: any[] = [];
  for (const item of Array.from(dt.items)) {
    const entry = (item as any).webkitGetAsEntry?.();
    if (entry) entries.push(entry);
  }
  const out: UploadItem[] = [];
  if (entries.length > 0) {
    for (const entry of entries) await walkEntry(entry, "", out);
  } else {
    for (const file of Array.from(dt.files)) out.push({ file, path: file.name });
  }
  return out;
}

// File/folder picker: webkitRelativePath carries the structure when a folder is chosen.
export function itemsFromFileList(files: FileList): UploadItem[] {
  return Array.from(files).map((file) => ({
    file,
    path: (file as any).webkitRelativePath || file.name,
  }));
}

export function fileExtension(path: string): string {
  return path.includes(".") ? path.split(".").pop()!.toLowerCase() : "";
}
