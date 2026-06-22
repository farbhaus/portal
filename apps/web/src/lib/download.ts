// Recipient-side download helpers. Portal only mints short-lived signed URLs; the browser
// fetches bytes directly from Frame.io's storage — nothing flows through Portal.

import { formatBytes as fmtBytes } from "$lib/format";

export type DownloadFile = {
  id: string;
  name: string;
  size: number | null;
  media_type: string | null;
  thumbnail_url: string | null;
  // Relative folder path ("" for flat sources). Used to recreate the subfolder tree when the
  // recipient saves into a chosen directory.
  path: string;
};

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function asJson(res: Response): Promise<any> {
  return res.json().catch(() => ({}));
}

export async function startSession(
  token: string,
  fields: { name?: string; email?: string; password?: string; code?: string },
): Promise<{ sessionId: string; files: DownloadFile[] }> {
  const res = await fetch(`/api/public/downloads/${token}/sessions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(fields),
  });
  if (!res.ok) throw new Error((await asJson(res)).detail ?? "Could not open this link");
  const body = await asJson(res);
  return { sessionId: body.session_id, files: body.files };
}

// Ask Portal to email a verification code (or report this device is already trusted).
export async function requestCode(
  token: string,
  email: string,
): Promise<{ trusted: boolean; sent: boolean }> {
  const res = await fetch(`/api/public/downloads/${token}/request-code`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email }),
  });
  if (!res.ok) throw new Error((await asJson(res)).detail ?? "Could not send a code");
  return asJson(res);
}

// Ask Portal for a fresh signed URL for one file (records the download event + cap).
async function signedUrl(token: string, sessionId: string, fileId: string): Promise<string> {
  const res = await fetch(
    `/api/public/downloads/${token}/sessions/${sessionId}/files/${fileId}/url`,
    { method: "POST" },
  );
  if (!res.ok) throw new Error((await asJson(res)).detail ?? "Download failed");
  return (await asJson(res)).url as string;
}

// Trigger a browser download by navigating a hidden iframe to the signed URL. Using an iframe
// (rather than location/anchor) lets us fire several sequential downloads without navigating away.
function triggerDownload(url: string): void {
  const iframe = document.createElement("iframe");
  iframe.style.display = "none";
  iframe.src = url;
  document.body.appendChild(iframe);
  setTimeout(() => iframe.remove(), 60_000);
}

export async function downloadFile(token: string, sessionId: string, fileId: string): Promise<void> {
  triggerDownload(await signedUrl(token, sessionId, fileId));
}

// "Download all" = sequential per-file downloads (no server-side ZIP). A small gap between files
// keeps browsers from dropping rapid-fire downloads.
export async function downloadAll(
  token: string,
  sessionId: string,
  files: DownloadFile[],
  onProgress?: (done: number, total: number) => void,
): Promise<void> {
  for (let i = 0; i < files.length; i++) {
    triggerDownload(await signedUrl(token, sessionId, files[i].id));
    onProgress?.(i + 1, files.length);
    if (i < files.length - 1) await sleep(800);
  }
}

// ── "Download all" into a recipient-chosen folder (Chromium File System Access API) ──────────
// Unlike the iframe path above, this fetches bytes into the page and streams them to disk, so it
// can recreate the source's subfolder tree and handle files larger than available memory. Only
// available where window.showDirectoryPicker exists (Chrome/Edge); callers fall back to
// downloadAll elsewhere.

// Minimal shapes for the File System Access API (not in the ambient TS lib here).
type FsWritable = {
  write(chunk: Uint8Array): Promise<void>;
  close(): Promise<void>;
  abort?(): Promise<void>;
};
type FsFileHandle = { createWritable(): Promise<FsWritable> };
type FsDirHandle = {
  name: string;
  getDirectoryHandle(name: string, opts?: { create?: boolean }): Promise<FsDirHandle>;
  getFileHandle(name: string, opts?: { create?: boolean }): Promise<FsFileHandle>;
};

// Live progress for a folder download. Because we bypass the browser's download manager, this is
// the only progress signal — the UI renders a bar/label from it. `bytesTotal` is null when any
// file's size is unknown (then show file-count progress instead of a byte bar).
export type FolderProgress = {
  fileIndex: number; // 0-based index of the file currently being written
  fileName: string;
  filesTotal: number;
  bytesDone: number; // cumulative bytes written across all files so far
  bytesTotal: number | null;
};

export type FolderResult = {
  dirName: string; // name of the folder the recipient picked (for the completion message)
  saved: number; // files written successfully
  failed: string[]; // names of files that couldn't be saved
};

export function supportsFolderPicker(): boolean {
  return typeof window !== "undefined" && typeof (window as any).showDirectoryPicker === "function";
}

// Resolve (creating as needed) the nested subdirectory for a file's path, caching handles so files
// sharing a folder don't re-resolve it.
async function dirForPath(
  root: FsDirHandle,
  path: string,
  cache: Map<string, FsDirHandle>,
): Promise<FsDirHandle> {
  if (!path) return root;
  const cached = cache.get(path);
  if (cached) return cached;
  let dir = root;
  let acc = "";
  for (const seg of path.split("/").filter(Boolean)) {
    acc = acc ? `${acc}/${seg}` : seg;
    const hit = cache.get(acc);
    if (hit) {
      dir = hit;
      continue;
    }
    dir = await dir.getDirectoryHandle(seg, { create: true });
    cache.set(acc, dir);
  }
  return dir;
}

// Prompt for a destination folder, then stream each file straight to disk under its relative path,
// reporting byte-level progress as it goes (we read the response in chunks and write each chunk, so
// memory stays flat even for files larger than RAM). Returns the picked folder name plus saved /
// failed counts. Throws AbortError if the recipient dismisses the picker (no work done) — callers
// should treat that as a no-op.
export async function downloadAllToFolder(
  token: string,
  sessionId: string,
  files: DownloadFile[],
  onProgress?: (p: FolderProgress) => void,
): Promise<FolderResult> {
  const root: FsDirHandle = await (window as any).showDirectoryPicker({ mode: "readwrite" });
  const dirCache = new Map<string, FsDirHandle>();
  const failed: string[] = [];
  // Total is known only if every file reports a size; otherwise fall back to count-based progress.
  const bytesTotal = files.every((f) => f.size != null)
    ? files.reduce((sum, f) => sum + (f.size ?? 0), 0)
    : null;
  let bytesDone = 0;

  for (let i = 0; i < files.length; i++) {
    const file = files[i];
    let writable: FsWritable | undefined;
    const report = () =>
      onProgress?.({
        fileIndex: i,
        fileName: file.name,
        filesTotal: files.length,
        bytesDone,
        bytesTotal,
      });
    report();
    try {
      const res = await fetch(await signedUrl(token, sessionId, file.id));
      if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);
      const dir = await dirForPath(root, file.path, dirCache);
      const handle = await dir.getFileHandle(file.name, { create: true });
      writable = await handle.createWritable();
      const reader = res.body.getReader();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        await writable.write(value);
        bytesDone += value.byteLength;
        report();
      }
      await writable.close();
    } catch {
      await writable?.abort?.().catch(() => {});
      failed.push(file.name);
    }
  }
  return { dirName: root.name, saved: files.length - failed.length, failed };
}

// Null-aware wrapper (a file size can be unknown); core formatting lives in format.ts.
export function formatBytes(n: number | null): string {
  return n === null ? "" : fmtBytes(n);
}
