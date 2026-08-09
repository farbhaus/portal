// "Download all" as a single ZIP, for browsers without the File System Access API.
//
// The archive is assembled by the service worker (src/service-worker.ts) and written straight to
// disk by the browser's own download manager; this module is only the page's half — capability
// detection, starting a job, and relaying the worker's progress back to the UI.

import { dev } from "$app/environment";
import { triggerDownload, type DownloadFile } from "$lib/download";
import { zipPaths } from "$lib/filetree";
import { KEEPALIVE_PATH, PROBE_PATH, PROBE_TITLE, ZIP_PREFIX } from "$lib/zip-protocol";
import type { ZipCancelMessage, ZipJobMessage, ZipWorkerMessage } from "$lib/zip-protocol";

/** Mirrors FolderProgress from download.ts so the page can render either path with one bar. */
export type ZipProgress = {
  fileIndex: number;
  fileName: string;
  filesTotal: number;
  bytesDone: number;
  bytesTotal: number | null;
};

export type ZipResult = { saved: number; failed: string[]; cancelled: boolean };

// client-zip writes no ZIP64 end-of-central-directory when the archive is under 4 GiB, and the
// classic record's file count is a uint16 — past 65535 entries it silently wraps and the archive is
// unreadable. Stay well clear and let such a link fall back to per-file downloads.
export const MAX_ZIP_ENTRIES = 60_000;
/** Survives Firefox's background-tab timer clamp; Chromium doesn't need it but it costs nothing. */
const KEEPALIVE_MS = 4_000;
// The worker reports the first file the moment it picks the job up, so silence past this means the
// request never reached it and no amount of waiting will help.
const START_TIMEOUT_MS = 10_000;
/** The probe is answered by the worker itself, so it either comes back at once or never. */
const PROBE_TIMEOUT_MS = 3_000;

/** Thrown when the archive never started — the caller should fall back to per-file downloads. */
export const ZIP_UNAVAILABLE = "ZipUnavailable";
export const isZipUnavailable = (e: unknown): boolean =>
  e instanceof Error && e.name === ZIP_UNAVAILABLE;

let ready: Promise<boolean> | null = null;
let worker: ServiceWorker | null = null;

/**
 * Can this browser take a streamed ZIP?
 *
 * The answer is settled by *probing*, not by inspecting state. The whole mechanism rests on one
 * thing — the worker intercepting a request from this page — and after a hard reload
 * (Cmd/Ctrl+Shift+R) Firefox keeps bypassing the worker for subdocument loads, so the download
 * iframe would 404 to the network and nothing would happen. Neither an active
 * registration nor a `controller` tells you that: only asking does. So we register, ask the worker
 * to claim the page if it isn't controlling it, then probe the download channel itself. If the
 * worker doesn't answer, fall back to per-file downloads rather than render a button that does
 * nothing.
 */
export function zipSupported(): Promise<boolean> {
  return (ready ??= (async () => {
    if (typeof navigator === "undefined" || !("serviceWorker" in navigator)) return false;
    if (!window.isSecureContext) return false;
    // iOS/iPadOS is deliberately excluded: per-tab memory ceilings are far tighter and the tab is
    // suspended when backgrounded, which kills a long archive. Recipients on a phone are fetching a
    // clip or two, not a dailies folder.
    if (/iP(hone|ad|od)/.test(navigator.platform ?? "") || isIpadOS()) return false;
    try {
      // Scoped to /d/ so the admin app is never routed through a worker. Kit serves the worker as
      // an ES module in dev and a classic script in a build — Firefox still has no module service
      // workers (bugzilla 1360870), so `vite dev` in Firefox falls back; production is fine.
      const reg = await navigator.serviceWorker.register("/service-worker.js", {
        scope: "/d/",
        type: dev ? "module" : "classic",
      });
      worker = reg.active ?? (await navigator.serviceWorker.ready).active;
      if (!worker) return false;
      if (!navigator.serviceWorker.controller) {
        worker.postMessage({ type: "portal-zip-claim" });
        await controllerChange(2_000);
      }
      return await intercepts();
    } catch {
      return false;
    }
  })());
}

/**
 * Does an *iframe navigation* from this page reach the worker? That is the only question that
 * matters, and it must be asked in exactly this form: a download is delivered by navigating a hidden
 * iframe, and after a hard reload Firefox keeps bypassing the worker for subdocument loads while
 * ordinary `fetch()` calls sail through. Probing with a fetch says yes and the download then does
 * nothing.
 *
 * Fails safe: if the request escapes to the network, Caddy stamps `X-Frame-Options: DENY` on the
 * 404, reading `contentDocument` throws, and we report blocked — which is the right answer.
 */
function intercepts(): Promise<boolean> {
  return new Promise((resolve) => {
    const frame = document.createElement("iframe");
    frame.style.display = "none";
    frame.src = PROBE_PATH;
    const settle = (ok: boolean) => {
      clearTimeout(timer);
      frame.remove();
      resolve(ok);
    };
    const timer = setTimeout(() => settle(false), PROBE_TIMEOUT_MS);
    frame.onload = () => {
      try {
        settle(frame.contentDocument?.title === PROBE_TITLE);
      } catch {
        settle(false);
      }
    };
    frame.onerror = () => settle(false);
    document.body.appendChild(frame);
  });
}

function controllerChange(ms: number): Promise<void> {
  return new Promise((resolve) => {
    const settle = () => {
      clearTimeout(timer);
      navigator.serviceWorker.removeEventListener("controllerchange", settle);
      resolve();
    };
    const timer = setTimeout(settle, ms);
    navigator.serviceWorker.addEventListener("controllerchange", settle);
  });
}

// iPadOS 13+ reports itself as a Mac; the touch-point count is the usual tell.
const isIpadOS = () =>
  navigator.platform === "MacIntel" && (navigator.maxTouchPoints ?? 0) > 1;

/** A filesystem-safe archive name (the link's or folder's display name). */
export function safeZipName(raw: string): string {
  const name = raw
    .replace(/[/\\?%*:|"<>\x00-\x1f]/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/^\.+/, "")
    .slice(0, 120)
    .trim();
  return name || "download";
}

/**
 * Start a streamed-ZIP download and resolve when the worker has finished feeding it.
 *
 * Order matters: the iframe navigation goes first and the job message second, both synchronously
 * inside the click's task — Safari gates downloads on user activation, and awaiting anything before
 * navigating would spend it. The worker parks the request until the message lands, so their arrival
 * order doesn't matter.
 */
export function downloadAllAsZip(
  token: string,
  sessionId: string,
  files: DownloadFile[],
  zipName: string,
  onProgress?: (p: ZipProgress) => void,
  signal?: AbortSignal,
): Promise<ZipResult> {
  const sw = worker;
  if (!sw) return Promise.reject(new Error("Download failed"));

  const jobId = crypto.randomUUID();
  const paths = zipPaths(files);
  const message: ZipJobMessage = {
    type: "portal-zip-job",
    jobId,
    token,
    sessionId,
    entries: files.map((f, i) => ({ id: f.id, name: paths[i], size: f.size })),
  };

  const settled = listen(jobId, onProgress);
  triggerDownload(`${ZIP_PREFIX}${jobId}/${encodeURIComponent(zipName)}.zip`);
  sw.postMessage(message);

  const cancel: ZipCancelMessage = { type: "portal-zip-cancel", jobId };
  const onAbort = () => sw.postMessage(cancel);
  signal?.addEventListener("abort", onAbort);

  // Firefox terminates a worker that is still feeding a download unless the page keeps it busy. The
  // fetch only reaches the worker while this page is controlled by it (a hard-reloaded page is
  // not), so also poke it with a message, which always lands.
  const ping = setInterval(() => {
    sw.postMessage({ type: "portal-zip-ping" });
    fetch(KEEPALIVE_PATH, { method: "POST", cache: "no-store" }).catch(() => {});
  }, KEEPALIVE_MS);

  return settled.finally(() => {
    clearInterval(ping);
    signal?.removeEventListener("abort", onAbort);
  });
}

/** Resolve/reject on the worker's verdict for one job, relaying progress meanwhile. */
function listen(jobId: string, onProgress?: (p: ZipProgress) => void): Promise<ZipResult> {
  return new Promise((resolve, reject) => {
    // If nothing at all comes back, the iframe never reached the worker (an inactive registration
    // 404s to the network instead). Fail loudly rather than leave the recipient watching a spinner.
    let started = false;
    const watchdog = setTimeout(() => {
      if (started) return;
      const err = new Error("Could not start the download.");
      err.name = ZIP_UNAVAILABLE;
      finish(() => reject(err));
    }, START_TIMEOUT_MS);

    function finish(settle: () => void) {
      clearTimeout(watchdog);
      navigator.serviceWorker.removeEventListener("message", onMessage);
      settle();
    }

    function onMessage(event: MessageEvent) {
      const msg = event.data as ZipWorkerMessage;
      if (!msg?.type?.startsWith("portal-zip-") || msg.jobId !== jobId) return;
      started = true;
      if (msg.type === "portal-zip-progress") {
        const { fileIndex, fileName, filesTotal, bytesDone, bytesTotal } = msg;
        onProgress?.({ fileIndex, fileName, filesTotal, bytesDone, bytesTotal });
      } else if (msg.type === "portal-zip-done") {
        finish(() => resolve({ saved: msg.saved, failed: msg.failed, cancelled: false }));
      } else if (msg.type === "portal-zip-cancelled") {
        finish(() => resolve({ saved: 0, failed: [], cancelled: true }));
      } else {
        finish(() => reject(new Error(msg.message || "Download failed")));
      }
    }

    navigator.serviceWorker.addEventListener("message", onMessage);
  });
}
