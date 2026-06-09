import { apiFetch } from "$lib/server/api";
import { error } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";
import type { DownloadLink } from "../+page.server";

export const load: PageServerLoad = async ({ params, request }) => {
  const res = await apiFetch(`/api/download-links/${params.id}`, request.headers.get("cookie"));
  if (!res.ok) throw error(res.status === 404 ? 404 : 500, "Download link not found");
  const link: DownloadLink = await res.json();
  return { link };
};
