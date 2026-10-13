import { redirect } from "@sveltejs/kit";
import type { PageLoad } from "./$types";

// Connections moved into Settings. Preserve any OAuth return params.
export const load: PageLoad = async ({ url }) => {
  const qs = url.search ? url.search : "";
  redirect(308, `/settings${qs}`);
};
