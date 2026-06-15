import { redirect } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";

export type Destination = {
  id: string;
  display_name: string;
  config: Record<string, string>;
  logo_url: string | null;
  accent_color: string | null;
  subtitle: string | null;
  created_at: string;
  updated_at: string;
};

export const load: PageServerLoad = async () => {
  throw redirect(302, "/sync");
};
