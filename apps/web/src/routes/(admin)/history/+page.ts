import type { PageLoad } from "./$types";

export type Transfer = {
  id: string;
  kind: string;
  label: string | null;
  who: string | null;
  bytes: number | null;
  status: string;
  at: string;
};

const PAGE_SIZE = 50;

export const load: PageLoad = async ({ fetch, url }) => {
  const kind = url.searchParams.get("kind") ?? "all";
  const offset = Math.max(0, parseInt(url.searchParams.get("offset") ?? "0", 10) || 0);
  const res = await fetch(
    `/api/dashboard/history?kind=${encodeURIComponent(kind)}&limit=${PAGE_SIZE}&offset=${offset}`,
  );
  const data = res.ok ? await res.json() : { items: [], has_more: false };
  return {
    items: data.items as Transfer[],
    hasMore: data.has_more as boolean,
    kind,
    offset,
    pageSize: PAGE_SIZE,
  };
};
