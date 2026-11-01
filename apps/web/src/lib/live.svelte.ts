// Live updates for admin pages. One EventSource on /api/events per tab; the server says "change"
// whenever uploads, downloads or links change, and pages re-run their loads then
// instead of polling on a timer. EventSource reconnects by itself after a drop.

import { invalidateAll } from "$app/navigation";

export const live = $state({ connected: false, version: 0 });

let source: EventSource | null = null;
const listeners = new Set<() => void>();

export function startLive() {
  if (source || typeof EventSource === "undefined") return;
  source = new EventSource("/api/events");
  source.addEventListener("ready", () => (live.connected = true));
  source.addEventListener("change", () => {
    live.connected = true;
    live.version++;
    for (const fn of listeners) fn();
  });
  source.onerror = () => (live.connected = false);
}

export function stopLive() {
  source?.close();
  source = null;
  live.connected = false;
}

/** Call `fn` on every live change; returns the unsubscribe (use as an $effect's cleanup). */
export function onLiveChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/**
 * Keep the current page fresh: re-run its loads on each live change (coalesced), and, while the
 * stream is down, every `fallbackMs` (0 = never) so nothing freezes if a proxy blocks streaming.
 * Returns the cleanup for an $effect.
 */
export function refreshOnChange(fallbackMs = 0): () => void {
  let pending = false;
  let busy = false;
  const run = async () => {
    if (busy) {
      pending = true;
      return;
    }
    busy = true;
    try {
      await invalidateAll();
    } finally {
      busy = false;
      if (pending) {
        pending = false;
        void run();
      }
    }
  };
  const off = onLiveChange(() => void run());
  const t = fallbackMs
    ? setInterval(() => {
        if (!live.connected && !document.hidden) void run();
      }, fallbackMs)
    : undefined;
  return () => {
    off();
    if (t) clearInterval(t);
  };
}
