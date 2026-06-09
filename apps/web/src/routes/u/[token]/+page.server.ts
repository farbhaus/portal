import { apiFetch } from "$lib/server/api";
import { error } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";

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
};

export const load: PageServerLoad = async ({ params }) => {
  // No cookie forwarded: this is a public, unauthenticated endpoint.
  const res = await apiFetch(`/api/public/links/${params.token}`, null);
  if (res.status === 404) throw error(404, "This upload link does not exist.");
  if (!res.ok) throw error(500, "Something went wrong.");
  const link: PublicLink = await res.json();
  return { token: params.token, link };
};
