import { redirect } from "@sveltejs/kit";
import { apiFetch } from "$lib/server/api";
import type { LayoutServerLoad } from "./$types";

export const load: LayoutServerLoad = async ({ request }) => {
  const res = await apiFetch("/api/auth/me", request.headers.get("cookie"));
  if (!res.ok) {
    redirect(302, "/login");
  }
  const user = (await res.json()) as { id: string; email: string };
  return { user };
};
