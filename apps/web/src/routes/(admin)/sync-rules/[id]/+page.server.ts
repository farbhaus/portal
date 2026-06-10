import { apiFetch } from "$lib/server/api";
import { error } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";
import type { SyncRule } from "../+page.server";

export type SyncJob = {
  id: string;
  frameio_file_id: string;
  status: string;
  bytes: number | null;
  sha256: string | null;
  error: string | null;
  retry_count: number;
  started_at: string | null;
  completed_at: string | null;
  created_at: string;
};

export const load: PageServerLoad = async ({ params, request }) => {
  const cookie = request.headers.get("cookie");
  const [ruleRes, jobsRes, statsRes] = await Promise.all([
    apiFetch(`/api/sync-rules/${params.id}`, cookie),
    apiFetch(`/api/sync-rules/${params.id}/jobs`, cookie),
    apiFetch(`/api/sync-rules/${params.id}/stats`, cookie),
  ]);
  if (!ruleRes.ok) throw error(ruleRes.status === 404 ? 404 : 500, "Sync rule not found");
  const rule: SyncRule = await ruleRes.json();
  const jobs: SyncJob[] = jobsRes.ok ? await jobsRes.json() : [];
  const stats: { counts: Record<string, number> } = statsRes.ok
    ? await statsRes.json()
    : { counts: {} };
  return { rule, jobs, stats };
};
