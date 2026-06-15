import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";
import type { Destination } from "../destinations/+page.server";
import type { SyncRule } from "../sync-rules/+page.server";

export type { Destination, SyncRule };

export const load: PageServerLoad = async ({ request }) => {
  const cookie = request.headers.get("cookie");
  const [destRes, rulesRes] = await Promise.all([
    apiFetch("/api/destinations", cookie),
    apiFetch("/api/sync-rules", cookie),
  ]);
  const destinations: Destination[] = destRes.ok ? await destRes.json() : [];
  const rules: SyncRule[] = rulesRes.ok ? await rulesRes.json() : [];
  return { destinations, rules };
};
