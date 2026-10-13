import { error } from "@sveltejs/kit";
import type { PageLoad } from "./$types";

export type PublicLink = {
  state: "ok" | "expired" | "revoked";
  display_name: string;
  subtitle: string | null;
  logo_url: string | null;
  accent_color: string | null;
  password_required: boolean;
  max_file_size: number | null;
  allowed_extensions: string[] | null;
  uploader_fields_required: { name: boolean; email: boolean; message: boolean };
  verify_email: boolean;
};

export const load: PageLoad = async ({ params, fetch }) => {
  // Public, unauthenticated endpoint: no session is involved.
  const res = await fetch(`/api/public/links/${params.token}`);
  if (res.status === 404) throw error(404, "This upload link does not exist.");
  if (!res.ok) throw error(500, "Something went wrong.");
  const link: PublicLink = await res.json();
  return { token: params.token, link };
};
