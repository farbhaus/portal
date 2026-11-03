import { error } from "@sveltejs/kit";
import type { PageLoad } from "./$types";

export type PublicDownload = {
  state: "ok" | "expired" | "revoked";
  expires_at: string | null;
  display_name: string;
  subtitle: string | null;
  logo_url: string | null;
  accent_color: string | null;
  password_required: boolean;
  viewer_fields_required: { name: boolean; email: boolean };
  verify_email: boolean;
  allow_preview: boolean;
  // Count and size up front, for sends whose files were stored with the link (not folders).
  summary: { files: number; bytes: number | null } | null;
};

export const load: PageLoad = async ({ params, fetch }) => {
  const res = await fetch(`/api/public/downloads/${params.token}`);
  if (res.status === 404) throw error(404, "This download link does not exist.");
  if (!res.ok) throw error(500, "Something went wrong.");
  const link: PublicDownload = await res.json();
  return { token: params.token, link };
};
