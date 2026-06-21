// Recipient-side download helpers. Portal only mints short-lived signed URLs; the browser
// fetches bytes directly from Frame.io's storage — nothing flows through Portal.

import { formatBytes as fmtBytes } from "$lib/format";

export type DownloadFile = {
  id: string;
  name: string;
  size: number | null;
  media_type: string | null;
  thumbnail_url: string | null;
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

// Null-aware wrapper (a file size can be unknown); core formatting lives in format.ts.
export function formatBytes(n: number | null): string {
  return n === null ? "" : fmtBytes(n);
}
