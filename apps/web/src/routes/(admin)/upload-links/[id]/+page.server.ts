import { apiFetch } from "$lib/server/api";
import { error } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";
import type { UploadLink } from "../+page.server";

export type UploadStats = {
  uploads: number;
  total_bytes: number;
  last_activity: string | null;
};

export const load: PageServerLoad = async ({ params, request }) => {
  const cookie = request.headers.get("cookie");
  const [linkRes, statsRes] = await Promise.all([
    apiFetch(`/api/upload-links/${params.id}`, cookie),
    apiFetch(`/api/upload-links/${params.id}/stats`, cookie),
  ]);
  if (!linkRes.ok) throw error(linkRes.status === 404 ? 404 : 500, "Upload link not found");
  const link: UploadLink = await linkRes.json();
  const stats: UploadStats = statsRes.ok
    ? await statsRes.json()
    : { uploads: 0, total_bytes: 0, last_activity: null };
  return { link, stats };
};
