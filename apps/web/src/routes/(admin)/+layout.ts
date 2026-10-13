import { redirect } from "@sveltejs/kit";
import type { LayoutLoad } from "./$types";

export const load: LayoutLoad = async ({ fetch }) => {
  const res = await fetch("/api/auth/me");
  if (!res.ok) {
    redirect(302, "/login");
  }
  const user = (await res.json()) as { id: string; email: string };
  return { user };
};
