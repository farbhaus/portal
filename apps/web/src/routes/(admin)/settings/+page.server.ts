import { apiFetch } from "$lib/server/api";
import type { PageServerLoad } from "./$types";

export type EmailStatus = {
  configured: boolean;
  smtp_host: string | null;
  smtp_from: string | null;
  notify_email: string;
};

export type FrameioStatus = {
  connected: boolean;
  adobe_email?: string | null;
  expires_at?: string | null;
  needs_refresh?: boolean | null;
  scopes?: string | null;
};

export const load: PageServerLoad = async ({ request }) => {
  const cookie = request.headers.get("cookie");
  const [emailRes, generalRes, frameioRes] = await Promise.all([
    apiFetch("/api/settings/email", cookie),
    apiFetch("/api/settings/general", cookie),
    apiFetch("/api/frameio/status", cookie),
  ]);
  const email: EmailStatus = emailRes.ok
    ? await emailRes.json()
    : { configured: false, smtp_host: null, smtp_from: null, notify_email: "" };
  const general: { base_url: string } = generalRes.ok ? await generalRes.json() : { base_url: "" };
  const frameio: FrameioStatus = frameioRes.ok ? await frameioRes.json() : { connected: false };
  return { email, general, frameio };
};
