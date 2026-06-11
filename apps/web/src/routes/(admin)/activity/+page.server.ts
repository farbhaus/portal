import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";

export type Activity = {
  id: string;
  action: string;
  user_email: string | null;
  detail: Record<string, unknown> | null;
  created_at: string;
};

export const load: PageServerLoad = async ({ request, url }) => {
  const cookie = request.headers.get("cookie");
  const action = url.searchParams.get("action") ?? "";
  const qs = action ? `?action=${encodeURIComponent(action)}` : "";
  const [eventsRes, actionsRes] = await Promise.all([
    apiFetch(`/api/activity${qs}`, cookie),
    apiFetch("/api/activity/actions", cookie),
  ]);
  const events: Activity[] = eventsRes.ok ? await eventsRes.json() : [];
  const actions: string[] = actionsRes.ok ? await actionsRes.json() : [];
  return { events, actions, action };
};
