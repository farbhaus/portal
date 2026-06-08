import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";

export type Destination = {
  id: string;
  display_name: string;
  config: Record<string, string>;
  logo_url: string | null;
  accent_color: string | null;
  subtitle: string | null;
  created_at: string;
  updated_at: string;
};

export const load: PageServerLoad = async ({ request }) => {
  const res = await apiFetch("/api/destinations", request.headers.get("cookie"));
  const destinations: Destination[] = res.ok ? await res.json() : [];
  return { destinations };
};
