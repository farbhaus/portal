import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";
import type { UploadLink } from "../upload-links/+page.server";
import type { DownloadLink } from "../download-links/+page.server";

export type { UploadLink, DownloadLink };

export const load: PageServerLoad = async ({ request }) => {
  const cookie = request.headers.get("cookie");
  const [upRes, downRes] = await Promise.all([
    apiFetch("/api/upload-links", cookie),
    apiFetch("/api/download-links", cookie),
  ]);
  const uploadLinks: UploadLink[] = upRes.ok ? await upRes.json() : [];
  const downloadLinks: DownloadLink[] = downRes.ok ? await downRes.json() : [];
  return { uploadLinks, downloadLinks };
};
