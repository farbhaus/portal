import { apiFetch } from "$lib/server/api";
import { error } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";
import type { Destination } from "../+page.server";

export const load: PageServerLoad = async ({ params, request }) => {
  const res = await apiFetch(`/api/destinations/${params.id}`, request.headers.get("cookie"));
  if (!res.ok) throw error(res.status === 404 ? 404 : 500, "Destination not found");
  const destination: Destination = await res.json();
  return { destination };
};
