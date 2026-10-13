import { error } from "@sveltejs/kit";
import type { PageLoad } from "./$types";
import type { SyncRule } from "../+page";

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

export const load: PageLoad = async ({ params, fetch }) => {
  const [ruleRes, jobsRes, statsRes] = await Promise.all([
    fetch(`/api/sync-rules/${params.id}`),
    fetch(`/api/sync-rules/${params.id}/jobs`),
    fetch(`/api/sync-rules/${params.id}/stats`),
  ]);
  if (!ruleRes.ok) throw error(ruleRes.status === 404 ? 404 : 500, "Sync rule not found");
  const rule: SyncRule = await ruleRes.json();
  const jobs: SyncJob[] = jobsRes.ok ? await jobsRes.json() : [];
  const stats: { counts: Record<string, number> } = statsRes.ok
    ? await statsRes.json()
    : { counts: {} };
  return { rule, jobs, stats };
};
