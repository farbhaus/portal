import { redirect } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";

export type DownloadLink = {
  id: string;
  token: string;
  public_url: string;
  state: "ok" | "expired" | "revoked";
  source: Record<string, unknown>;
  expires_at: string | null;
  password_protected: boolean;
  max_downloads: number | null;
  downloads_count: number;
  viewer_fields_required: { name: boolean; email: boolean };
  verify_email: boolean;
  allow_preview: boolean;
  brand_logo_url: string | null;
  brand_accent_color: string | null;
  brand_display_name: string | null;
  brand_subtitle: string | null;
  revoked_at: string | null;
  created_at: string;
};

export const load: PageServerLoad = async () => {
  throw redirect(302, "/links");
};
