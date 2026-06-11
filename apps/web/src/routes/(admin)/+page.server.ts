import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";
import type { UploadLink } from "./upload-links/+page.server";
import type { DownloadLink } from "./download-links/+page.server";

export type ActiveUpload = {
  id: string;
  who: string | null;
  brand: string | null;
  total_bytes: number | null;
  started_at: string | null;
};

export type RecentTransfer = {
  id: string;
  kind: "upload" | "download" | "sync";
  label: string | null;
  who: string | null;
  bytes: number | null;
  status: string;
  at: string;
};

export type SyncHealth = {
  waiting: number;
  running: number;
  dead_letter: number;
  done_24h: number;
};

export type Transfers = {
  active_uploads: ActiveUpload[];
  recent: RecentTransfer[];
  sync_health: SyncHealth;
};

const EMPTY: Transfers = {
  active_uploads: [],
  recent: [],
  sync_health: { waiting: 0, running: 0, dead_letter: 0, done_24h: 0 },
};

export const load: PageServerLoad = async ({ request }) => {
  const cookie = request.headers.get("cookie");
  const [transfersRes, upRes, downRes, connRes] = await Promise.all([
    apiFetch("/api/dashboard/transfers?limit=15", cookie),
    apiFetch("/api/upload-links", cookie),
    apiFetch("/api/download-links", cookie),
    apiFetch("/api/frameio/status", cookie),
  ]);

  const transfers: Transfers = transfersRes.ok ? await transfersRes.json() : EMPTY;
  const uploadLinks: UploadLink[] = upRes.ok ? await upRes.json() : [];
  const downloadLinks: DownloadLink[] = downRes.ok ? await downRes.json() : [];
  const connected: boolean = connRes.ok ? (await connRes.json()).connected : false;

  return {
    transfers,
    activeUpload: uploadLinks.filter((l) => l.state === "ok"),
    activeDownload: downloadLinks.filter((l) => l.state === "ok"),
    connected,
  };
};
