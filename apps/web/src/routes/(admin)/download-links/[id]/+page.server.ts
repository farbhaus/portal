import { apiFetch } from "$lib/server/api";
import { error } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";
import type { DownloadLink } from "../+page.server";

export type DownloadEvent = {
  frameio_file_id: string;
  file_name: string | null;
  viewer_name: string | null;
  viewer_email: string | null;
  ip: string | null;
  completed: boolean;
  started_at: string;
};

export type DownloadStats = {
  downloads: number;
  unique_viewers: number;
  last_activity: string | null;
};

export const load: PageServerLoad = async ({ params, request }) => {
  const cookie = request.headers.get("cookie");
  const [linkRes, eventsRes, statsRes] = await Promise.all([
    apiFetch(`/api/download-links/${params.id}`, cookie),
    apiFetch(`/api/download-links/${params.id}/events`, cookie),
    apiFetch(`/api/download-links/${params.id}/stats`, cookie),
  ]);
  if (!linkRes.ok) throw error(linkRes.status === 404 ? 404 : 500, "Download link not found");
  const link: DownloadLink = await linkRes.json();
  const events: DownloadEvent[] = eventsRes.ok ? await eventsRes.json() : [];
  const stats: DownloadStats = statsRes.ok
    ? await statsRes.json()
    : { downloads: 0, unique_viewers: 0, last_activity: null };
  return { link, events, stats };
};
