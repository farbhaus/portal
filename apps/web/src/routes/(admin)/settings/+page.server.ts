import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";

export type EmailStatus = {
  configured: boolean;
  smtp_host: string | null;
  smtp_from: string | null;
  notify_email: string;
};

export const load: PageServerLoad = async ({ request }) => {
  const cookie = request.headers.get("cookie");
  const [emailRes, generalRes] = await Promise.all([
    apiFetch("/api/settings/email", cookie),
    apiFetch("/api/settings/general", cookie),
  ]);
  const email: EmailStatus = emailRes.ok
    ? await emailRes.json()
    : { configured: false, smtp_host: null, smtp_from: null, notify_email: "" };
  const general: { base_url: string } = generalRes.ok ? await generalRes.json() : { base_url: "" };
  return { email, general };
};
