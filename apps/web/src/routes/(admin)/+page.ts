import type { PageLoad } from "./$types";
import type { UploadLink } from "./upload-links/+page";
import type { DownloadLink } from "./download-links/+page";

export type ActiveUpload = {
  id: string;
  who: string | null;
  brand: string | null;
  uploaded_bytes: number | null;
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
  dead_letter_rule_id?: string | null;
};

export type Transfers = {
  active_uploads: ActiveUpload[];
  recent: RecentTransfer[];
  sync_health: SyncHealth;
};

const EMPTY: Transfers = {
  active_uploads: [],
  recent: [],
  sync_health: { waiting: 0, running: 0, dead_letter: 0, done_24h: 0, dead_letter_rule_id: null },
};

export const load: PageLoad = async ({ fetch }) => {
  const [transfersRes, upRes, downRes, connRes] = await Promise.all([
    fetch("/api/dashboard/transfers?limit=15"),
    fetch("/api/upload-links"),
    fetch("/api/download-links"),
    fetch("/api/frameio/status"),
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
