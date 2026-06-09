import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";

export type EmailStatus = {
  configured: boolean;
  smtp_host: string | null;
  smtp_from: string | null;
  notify_email: string;
};

export const load: PageServerLoad = async ({ request }) => {
  const res = await apiFetch("/api/settings/email", request.headers.get("cookie"));
  const email: EmailStatus = res.ok
    ? await res.json()
    : { configured: false, smtp_host: null, smtp_from: null, notify_email: "" };
  return { email };
};
