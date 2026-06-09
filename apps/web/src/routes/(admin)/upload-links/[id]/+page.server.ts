import { apiFetch } from "$lib/server/api";
import { error } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";
import type { UploadLink } from "../+page.server";

export const load: PageServerLoad = async ({ params, request }) => {
  const res = await apiFetch(`/api/upload-links/${params.id}`, request.headers.get("cookie"));
  if (!res.ok) throw error(res.status === 404 ? 404 : 500, "Upload link not found");
  const link: UploadLink = await res.json();
  return { link };
};
