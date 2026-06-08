import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";

type Status = {
  connected: boolean;
  adobe_email?: string | null;
  expires_at?: string | null;
  needs_refresh?: boolean | null;
  scopes?: string | null;
};

export const load: PageServerLoad = async ({ request }) => {
  const res = await apiFetch("/api/frameio/status", request.headers.get("cookie"));
  const status: Status = res.ok ? await res.json() : { connected: false };
  return { status };
};
