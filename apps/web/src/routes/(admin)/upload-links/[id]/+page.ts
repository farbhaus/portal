import { error } from "@sveltejs/kit";
import type { PageLoad } from "./$types";
import type { UploadLink } from "../+page";
import type { Destination } from "../../destinations/+page";

export type UploadStats = {
  uploads: number;
  total_bytes: number;
  last_activity: string | null;
};

export const load: PageLoad = async ({ params, fetch }) => {
  const [linkRes, statsRes, destRes] = await Promise.all([
    fetch(`/api/upload-links/${params.id}`),
    fetch(`/api/upload-links/${params.id}/stats`),
    fetch("/api/destinations"),
  ]);
  if (!linkRes.ok) throw error(linkRes.status === 404 ? 404 : 500, "Upload link not found");
  const link: UploadLink = await linkRes.json();
  const stats: UploadStats = statsRes.ok
    ? await statsRes.json()
    : { uploads: 0, total_bytes: 0, last_activity: null };
  const destinations: Destination[] = destRes.ok ? await destRes.json() : [];
  return { link, stats, destinations };
};
