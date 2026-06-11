import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";
import type { Activity } from "./activity/+page.server";

export type Summary = {
  destinations: number;
  upload_links: number;
  download_links: number;
  sync_rules: number;
};

export const load: PageServerLoad = async ({ request }) => {
  const cookie = request.headers.get("cookie");
  const [summaryRes, activityRes, connRes] = await Promise.all([
    apiFetch("/api/activity/summary", cookie),
    apiFetch("/api/activity?limit=8", cookie),
    apiFetch("/api/frameio/status", cookie),
  ]);
  const summary: Summary = summaryRes.ok
    ? await summaryRes.json()
    : { destinations: 0, upload_links: 0, download_links: 0, sync_rules: 0 };
  const recent: Activity[] = activityRes.ok ? await activityRes.json() : [];
  const connected: boolean = connRes.ok ? (await connRes.json()).connected : false;
  return { summary, recent, connected };
};
