import { error } from "@sveltejs/kit";
import type { PageLoad } from "./$types";

export type PublicDownload = {
  state: "ok" | "expired" | "revoked";
  display_name: string;
  subtitle: string | null;
  logo_url: string | null;
  accent_color: string | null;
  password_required: boolean;
  viewer_fields_required: { name: boolean; email: boolean };
  verify_email: boolean;
  allow_preview: boolean;
};

export const load: PageLoad = async ({ params, fetch }) => {
  const res = await fetch(`/api/public/downloads/${params.token}`);
  if (res.status === 404) throw error(404, "This download link does not exist.");
  if (!res.ok) throw error(500, "Something went wrong.");
  const link: PublicDownload = await res.json();
  return { token: params.token, link };
};
