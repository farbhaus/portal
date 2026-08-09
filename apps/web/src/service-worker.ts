/// <reference lib="esnext" />
/// <reference lib="webworker" />

// Streams a ZIP of a download link's files straight to the recipient's disk.
//
// Browsers other than Chromium have no File System Access API, so there is no way to write a folder
// tree to disk from a page. What every browser *can* do is save a download — so we synthesise one:
// the page points a hidden iframe at a URL under /d/_zip/, this worker answers that navigation with
// a `Content-Disposition: attachment` response whose body is a ZIP we build on the fly, and the
// browser writes it to disk as it arrives. Memory stays flat no matter how large the archive is.
//
// Portal never sees a byte: the worker mints each signed URL from the API and then pulls the file
// itself straight from Frame.io's storage, exactly as the page's Chromium path does.

import { makeZip, predictLength } from "client-zip";
import { KEEPALIVE_PATH, PROBE_PATH, PROBE_TITLE, ZIP_PREFIX } from "./lib/zip-protocol";
import type { ZipEntry, ZipJob, ZipWorkerMessage } from "./lib/zip-protocol";

const sw = self as unknown as ServiceWorkerGlobalScope;

/** How long a fetch for an unknown job waits for its postMessage to land (see `claimJob`). */
const JOB_WAIT_MS = 15_000;
/** Attempts to mint one signed URL, and to resume one interrupted file. */
const MINT_ATTEMPTS = 3;
const RESUME_ATTEMPTS = 3;
const MINT_BACKOFF_MS = [500, 2_000, 5_000];
/** Progress messages are throttled — a 40 GB archive is millions of chunks. */
const PROGRESS_MS = 250;

// Nothing to cache and nothing in flight to lose: a new worker only ever replaces an idle one on a
// fresh page load. `clients.claim()` matters more than it looks — the keep-alive ping is a fetch
// from the page, and an *uncontrolled* page's fetches bypass the worker entirely.
sw.addEventListener("install", () => sw.skipWaiting());
sw.addEventListener("activate", (event) => event.waitUntil(sw.clients.claim()));

// ── Job registry ─────────────────────────────────────────────────────────────────────────────────
// The page fires the iframe navigation and the job message in the same task, but their delivery
// order here is not guaranteed — and a cold worker may well be started *by* that navigation. So a
// fetch for an unknown job parks until the message arrives instead of 404-ing.

const jobs = new Map<string, ZipJob>();
const waiting = new Map<string, (job: ZipJob) => void>();
/** Archives currently being streamed, so the page can call one off. */
const running = new Map<string, AbortController>();

sw.addEventListener("message", (event) => {
  const data = event.data;
  if (data?.type === "portal-zip-cancel") {
    running.get(data.jobId)?.abort();
    return;
  }
  // A hard reload (Cmd/Ctrl+Shift+R) loads the page outside this worker's control. Claiming pulls
  // it back in, which is what lets the download iframe be intercepted at all.
  if (data?.type === "portal-zip-claim") {
    event.waitUntil(sw.clients.claim());
    return;
  }
  if (data?.type !== "portal-zip-job") return;
  const { jobId, token, sessionId, entries } = data as ZipJob & { type: string };
  const job: ZipJob = { jobId, token, sessionId, entries };
  jobs.set(jobId, job);
  waiting.get(jobId)?.(job);
});

function claimJob(jobId: string): Promise<ZipJob | null> {
  const known = jobs.get(jobId);
  if (known) return Promise.resolve(known);
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      waiting.delete(jobId);
      resolve(null);
    }, JOB_WAIT_MS);
    waiting.set(jobId, (job) => {
      clearTimeout(timer);
      waiting.delete(jobId);
      resolve(job);
    });
  });
}

function post(message: ZipWorkerMessage): void {
  // `includeUncontrolled` is essential, not incidental: after a hard reload (Cmd/Ctrl+Shift+R) the
  // page is outside this worker's control, and without it every progress and completion message
  // would be dropped — the archive would download fine while the page sat on a dead progress bar.
  sw.clients.matchAll({ type: "window", includeUncontrolled: true }).then((clients) => {
    for (const client of clients) client.postMessage(message);
  });
}

// ── Fetching one file ────────────────────────────────────────────────────────────────────────────

/** A failure we can still recover from by skipping the file — it produced no bytes. */
class FileUnavailable extends Error {}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/**
 * Ask Portal for a fresh signed URL. Frame.io's media links carry no expiry we can read
 * (`DownloadURL.expires_at` is always null), so a URL is minted immediately before its bytes are
 * read and never reused across a retry. Same-origin with credentials, like the page's own calls.
 */
async function mintUrl(job: ZipJob, fileId: string, signal: AbortSignal): Promise<string> {
  let last = "Download failed";
  for (let attempt = 0; attempt < MINT_ATTEMPTS; attempt++) {
    if (attempt) await sleep(MINT_BACKOFF_MS[attempt - 1]);
    signal.throwIfAborted();
    let res: Response;
    try {
      res = await fetch(
        `/api/public/downloads/${job.token}/sessions/${job.sessionId}/files/${fileId}/url`,
        { method: "POST", credentials: "same-origin", signal },
      );
    } catch (e) {
      if (signal.aborted) throw e; // cancelled, not a flaky network
      last = "Network error";
      continue; // transient: retry
    }
    if (res.ok) return (await res.json()).url as string;
    const detail = await res
      .json()
      .then((b) => b?.detail)
      .catch(() => null);
    last = detail ?? `HTTP ${res.status}`;
    // A download cap, a dead session or a vanished file will not get better by asking again.
    if (res.status !== 502 && res.status !== 503 && res.status !== 504) break;
  }
  throw new FileUnavailable(last);
}

/** Mint and open a file, throwing `FileUnavailable` if it never starts. */
async function openFile(
  job: ZipJob,
  entry: ZipEntry,
  signal: AbortSignal,
  from = 0,
): Promise<Response> {
  const url = await mintUrl(job, entry.id, signal);
  const res = await fetch(url, {
    signal,
    ...(from ? { headers: { Range: `bytes=${from}-` } } : {}),
  }).catch(() => null);
  if (signal.aborted) throw new DOMException("Cancelled", "AbortError");
  if (!res || !res.ok || !res.body) throw new FileUnavailable(`HTTP ${res?.status ?? "network"}`);
  // A resume that comes back 200 means the range was ignored and the body restarts from zero —
  // appending it would duplicate bytes, so treat it as unrecoverable rather than corrupt the entry.
  if (from && res.status !== 206) throw new Error("Range not supported");
  return res;
}

/**
 * One file's bytes. If the transfer dies part-way we re-mint (an expired URL is usually *why* it
 * died) and resume with a Range request, so a drop at 4 GB of a 5 GB file costs seconds, not the
 * whole file. Once the first chunk is out, failure is fatal for the archive: client-zip has already
 * written this entry's header, so the file can no longer be skipped, only truncated.
 */
async function* fileBytes(
  job: ZipJob,
  entry: ZipEntry,
  first: Response,
  signal: AbortSignal,
  onChunk: (n: number) => void,
): AsyncGenerator<Uint8Array> {
  let res = first;
  let read = 0;
  for (let attempt = 0; ; attempt++) {
    try {
      const reader = res.body!.getReader();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) return;
        read += value.byteLength;
        onChunk(value.byteLength);
        yield value;
      }
    } catch (e) {
      if (signal.aborted || attempt >= RESUME_ATTEMPTS) throw e;
      await sleep(MINT_BACKOFF_MS[Math.min(attempt, MINT_BACKOFF_MS.length - 1)]);
      res = await openFile(job, entry, signal, read);
    }
  }
}

// ── Assembling the archive ───────────────────────────────────────────────────────────────────────

/**
 * The entries fed to client-zip, opened one at a time so each signed URL is minted the moment its
 * bytes are needed.
 *
 * `strict` mirrors "we committed to a Content-Length": a skipped file would make the promised byte
 * count a lie, so any failure aborts the stream and the browser marks the download failed — which
 * is more honest than a silently short archive. When the length is unknown we can afford to be
 * lenient: skip what we can't fetch and finish with a _FAILED_FILES.txt listing it.
 */
async function* zipEntries(
  job: ZipJob,
  strict: boolean,
  failed: string[],
  signal: AbortSignal,
  onChunk: (n: number) => void,
  onFile: (index: number, entry: ZipEntry) => void,
) {
  for (let i = 0; i < job.entries.length; i++) {
    const entry = job.entries[i];
    signal.throwIfAborted();
    onFile(i, entry);
    let opened: Response;
    try {
      opened = await openFile(job, entry, signal);
    } catch (e) {
      if (signal.aborted) throw e;
      // Nothing written for this entry yet, so skipping it still leaves a valid archive.
      if (strict) throw new Error(`${entry.name}: ${(e as Error).message}`);
      failed.push(entry.name);
      continue;
    }
    yield {
      name: entry.name,
      size: entry.size ?? undefined,
      input: fileBytes(job, entry, opened, signal, onChunk),
    };
  }
  if (failed.length)
    yield {
      name: "_FAILED_FILES.txt",
      input:
        "These files could not be downloaded and are missing from this archive:\n\n" +
        failed.map((n) => `  ${n}`).join("\n") +
        "\n\nOpen the download link again to retry them individually.\n",
    };
}

/** RFC 5987 both ways: WebKit has a long-standing bug with non-ASCII `filename` (webkit 20407). */
function contentDisposition(name: string): string {
  const ascii = name.replace(/[^\x20-\x7e]/g, "_").replace(/["\\]/g, "");
  return `attachment; filename="${ascii}"; filename*=UTF-8''${encodeURIComponent(name)}`;
}

async function zipResponse(url: URL): Promise<Response> {
  const [jobId, filename] = url.pathname.slice(ZIP_PREFIX.length).split("/");
  const job = await claimJob(jobId);
  if (!job) return new Response("This download expired. Please try again.", { status: 410 });
  jobs.delete(jobId); // one-shot: a reloaded iframe must not restart the archive
  const abort = new AbortController();
  running.set(jobId, abort);

  // A store-only ZIP's size is exactly predictable when every file's size is known — worth the
  // trouble, because it is what turns the browser's own download manager into a real progress bar
  // with an ETA. One unknown size and we stream with no length instead (and go lenient, above).
  const sizes = job.entries.every((e) => e.size !== null);
  const length = sizes
    ? predictLength(job.entries.map((e) => ({ name: e.name, size: e.size! })))
    : null;

  const filesTotal = job.entries.length;
  const bytesTotal = sizes ? job.entries.reduce((n, e) => n + e.size!, 0) : null;
  const failed: string[] = [];
  let bytesDone = 0;
  let fileIndex = 0;
  let fileName = job.entries[0]?.name ?? "";
  let last = 0;
  const report = (force = false) => {
    const now = Date.now();
    if (!force && now - last < PROGRESS_MS) return;
    last = now;
    post({
      type: "portal-zip-progress",
      jobId,
      fileIndex,
      fileName,
      filesTotal,
      bytesDone,
      bytesTotal,
    });
  };

  const entries = zipEntries(
    job,
    length !== null,
    failed,
    abort.signal,
    (n) => {
      bytesDone += n;
      report();
    },
    (index, entry) => {
      fileIndex = index;
      fileName = entry.name;
      report(true);
    },
  );

  // Relay the archive so the page hears how it ended. The browser's download manager knows, but the
  // recipient is looking at our page — and a strict-mode abort needs to name the file that failed.
  const reader = makeZip(entries, { buffersAreUTF8: true }).getReader();
  const done = (message: ZipWorkerMessage) => {
    running.delete(jobId);
    post(message);
  };
  const body = new ReadableStream<Uint8Array>({
    async pull(controller) {
      try {
        const { done: finished, value } = await reader.read();
        if (finished) {
          controller.close();
          done({ type: "portal-zip-done", jobId, saved: filesTotal - failed.length, failed });
          return;
        }
        controller.enqueue(value);
      } catch (e) {
        // An abort is the recipient's own doing, not a failure to report as one.
        done(
          abort.signal.aborted
            ? { type: "portal-zip-cancelled", jobId }
            : { type: "portal-zip-error", jobId, message: (e as Error).message ?? "Download failed" },
        );
        controller.error(e);
      }
    },
    // Reached when the recipient cancels in the browser's own download manager — without this the
    // page would sit on a progress bar forever, waiting for a stream nobody is reading any more.
    cancel(reason) {
      abort.abort();
      done({ type: "portal-zip-cancelled", jobId });
      return reader.cancel(reason);
    },
  });

  const headers = new Headers({
    "Content-Type": "application/zip",
    "Content-Disposition": contentDisposition(decodeURIComponent(filename || "download.zip")),
    "Cache-Control": "no-store",
  });
  if (length !== null) headers.set("Content-Length", String(length));

  return new Response(body, { headers });
}

sw.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (url.origin !== sw.location.origin) return; // never touch Frame.io's storage or anything else
  if (url.pathname === KEEPALIVE_PATH) return event.respondWith(new Response(null, { status: 204 }));
  // Must be matched before the job branch below, or claimJob would park on a job id of "probe".
  if (url.pathname === PROBE_PATH)
    return event.respondWith(
      new Response(`<!doctype html><title>${PROBE_TITLE}</title>`, {
        headers: { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" },
      }),
    );
  if (!url.pathname.startsWith(ZIP_PREFIX)) return; // everything else: ordinary network, untouched
  event.respondWith(zipResponse(url));
});
