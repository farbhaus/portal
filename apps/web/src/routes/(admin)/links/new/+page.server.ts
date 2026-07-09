import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";
import type { Destination } from "../../destinations/+page.server";

export const load: PageServerLoad = async ({ request }) => {
  const cookie = request.headers.get("cookie");
  const [destRes, presetRes] = await Promise.all([
    apiFetch("/api/destinations", cookie),
    apiFetch("/api/upload-links/extension-preset", cookie),
  ]);
  const destinations: Destination[] = destRes.ok ? await destRes.json() : [];
  const extensionPreset: string[] = presetRes.ok ? await presetRes.json() : [];
  return { destinations, extensionPreset };
};
