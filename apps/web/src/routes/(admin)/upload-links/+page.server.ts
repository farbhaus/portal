import { redirect } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";

export type UploadLink = {
  id: string;
  token: string;
  public_url: string;
  state: "ok" | "expired" | "revoked";
  destination_id: string;
  destination_name: string;
  expires_at: string | null;
  password_protected: boolean;
  max_file_size: number | null;
  allowed_extensions: string[] | null;
  uploader_fields_required: { name: boolean; email: boolean; message: boolean };
  verify_email: boolean;
  target_folder_id: string | null;
  target_folder_name: string | null;
  subfolder_template: string | null;
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
