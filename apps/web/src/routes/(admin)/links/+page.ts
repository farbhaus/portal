import type { PageLoad } from "./$types";
import type { UploadLink } from "../upload-links/+page";
import type { DownloadLink } from "../download-links/+page";

export type { UploadLink, DownloadLink };

export const load: PageLoad = async ({ fetch }) => {
  const [upRes, downRes] = await Promise.all([
    fetch("/api/upload-links"),
    fetch("/api/download-links"),
  ]);
  const uploadLinks: UploadLink[] = upRes.ok ? await upRes.json() : [];
  const downloadLinks: DownloadLink[] = downRes.ok ? await downRes.json() : [];
  return { uploadLinks, downloadLinks };
};
