import type { PageLoad } from "./$types";
import type { Destination } from "../destinations/+page";
import type { SyncRule } from "../sync-rules/+page";

export type { Destination, SyncRule };

export const load: PageLoad = async ({ fetch }) => {
  const [destRes, rulesRes] = await Promise.all([
    fetch("/api/destinations"),
    fetch("/api/sync-rules"),
  ]);
  const destinations: Destination[] = destRes.ok ? await destRes.json() : [];
  const rules: SyncRule[] = rulesRes.ok ? await rulesRes.json() : [];
  return { destinations, rules };
};
