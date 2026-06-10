import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";

export type SyncRule = {
  id: string;
  name: string;
  source: Record<string, unknown>;
  destination_path: string;
  conflict_policy: string;
  path_template: string | null;
  enabled: boolean;
  subscription_active: boolean;
  created_at: string;
};

export const load: PageServerLoad = async ({ request }) => {
  const res = await apiFetch("/api/sync-rules", request.headers.get("cookie"));
  const rules: SyncRule[] = res.ok ? await res.json() : [];
  return { rules };
};
