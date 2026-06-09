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
  fields: { name?: string; email?: string; message?: string; password?: string },
): Promise<string> {
  const res = await fetch(`/api/public/links/${token}/sessions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(fields),
  });
  if (!res.ok) throw new Error((await asJson(res)).detail ?? "Could not start the upload");
  return (await asJson(res)).session_id as string;
}

export async function completeSession(
  token: string,
  sessionId: string,
  totalBytes: number,
): Promise<void> {
  await fetch(`/api/public/links/${token}/sessions/${sessionId}/complete`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ total_bytes: totalBytes }),
  });
}

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
      } else reject(new Error(`Upload failed (${xhr.status})`));
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
      if (attempt === MAX_RETRIES - 1) throw err;
      await sleep(1000 * 2 ** attempt); // exponential backoff (resume after a disconnect)
    }
  }
}

async function pool<T>(items: T[], limit: number, worker: (item: T) => Promise<void>): Promise<void> {
  let i = 0;
  const runners = Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (i < items.length) {
      const idx = i++;
      await worker(items[idx]);
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

async function uploadOne(
  token: string,
  sessionId: string,
  item: UploadItem,
  onBytes: (delta: number) => void,
): Promise<void> {
  const res = await fetch(`/api/public/links/${token}/sessions/${sessionId}/files`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ path: item.path, size: item.file.size }),
  });
  if (!res.ok) throw new Error((await asJson(res)).detail ?? `Could not upload ${item.path}`);
  const result = (await asJson(res)) as FileCreateResult;

  let offset = 0;
  const jobs = result.chunks.map((chunk) => {
    const blob = item.file.slice(offset, offset + chunk.size);
    offset += chunk.size;
    return { chunk, blob };
  });
  await pool(jobs, CHUNK_CONCURRENCY, ({ chunk, blob }) =>
    uploadChunkWithRetry(chunk, blob, result.headers, onBytes),
  );
  await pollComplete(token, sessionId, result.file_id);
}

export type ProgressHandlers = {
  onBytes: (delta: number) => void;
  onFileStart: (path: string) => void;
  onFileDone: (path: string) => void;
};

// Upload files one at a time (chunks within a file go in parallel) so a huge drop doesn't open
// hundreds of sockets. Returns when every file is uploaded and the session is completed.
export async function runUpload(
  token: string,
  sessionId: string,
  items: UploadItem[],
  handlers: ProgressHandlers,
): Promise<void> {
  for (const item of items) {
    handlers.onFileStart(item.path);
    await uploadOne(token, sessionId, item, handlers.onBytes);
    handlers.onFileDone(item.path);
  }
  const total = items.reduce((sum, it) => sum + it.file.size, 0);
  await completeSession(token, sessionId, total);
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

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}
