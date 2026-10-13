import { error } from "@sveltejs/kit";
import type { PageLoad } from "./$types";
import type { Destination } from "../+page";

export const load: PageLoad = async ({ params, fetch }) => {
  const res = await fetch(`/api/destinations/${params.id}`);
  if (!res.ok) throw error(res.status === 404 ? 404 : 500, "Destination not found");
  const destination: Destination = await res.json();
  return { destination };
};
