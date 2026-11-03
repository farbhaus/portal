import type { PageLoad } from "./$types";
import type { LinkItem } from "$lib/links";

export type ActiveUpload = {
  id: string;
  who: string | null;
  brand: string | null;
  uploaded_bytes: number | null;
  total_bytes: number | null;
  started_at: string | null;
  link_id?: string;
  last_activity_at?: string | null;
};

export type Transfers = {
  active_uploads: ActiveUpload[];
};

export type HistoryItem = {
  id: string;
  kind: "upload" | "download";
  label: string | null;
  who: string | null;
  project?: string | null;
  bytes: number | null;
  files?: number;
  status: string;
  at: string;
  link_id?: string;
  via?: string | null;
};

const EMPTY: Transfers = { active_uploads: [] };

export const load: PageLoad = async ({ fetch }) => {
  const [transfersRes, linksRes, connRes] = await Promise.all([
    fetch("/api/dashboard/transfers?limit=1"),
    fetch("/api/links"),
    fetch("/api/frameio/status"),
  ]);
  const transfers: Transfers = transfersRes.ok ? { ...EMPTY, ...(await transfersRes.json()) } : EMPTY;
  const links: LinkItem[] = linksRes.ok ? await linksRes.json() : [];
  const connected: boolean = connRes.ok ? (await connRes.json()).connected : false;
  return { transfers, links, connected };
};
