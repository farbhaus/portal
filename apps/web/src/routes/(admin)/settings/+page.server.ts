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

export type Passkey = {
  id: string;
  name: string | null;
  created_at: string;
  last_used_at: string | null;
};
export type SecurityStatus = { totp_enabled: boolean; passkeys: Passkey[] };
export type BrandingStatus = {
  has_logo: boolean;
  logo_updated_at: string | null;
  brand_display_name: string | null;
  brand_accent_color: string | null;
};

export const load: PageServerLoad = async ({ request }) => {
  const cookie = request.headers.get("cookie");
  const [emailRes, generalRes, frameioRes, securityRes, brandingRes] = await Promise.all([
    apiFetch("/api/settings/email", cookie),
    apiFetch("/api/settings/general", cookie),
    apiFetch("/api/frameio/status", cookie),
    apiFetch("/api/settings/security", cookie),
    apiFetch("/api/settings/branding", cookie),
  ]);
  const email: EmailStatus = emailRes.ok
    ? await emailRes.json()
    : { configured: false, smtp_host: null, smtp_from: null, notify_email: "" };
  const general: { base_url: string } = generalRes.ok ? await generalRes.json() : { base_url: "" };
  const frameio: FrameioStatus = frameioRes.ok ? await frameioRes.json() : { connected: false };
  const security: SecurityStatus = securityRes.ok
    ? await securityRes.json()
    : { totp_enabled: false, passkeys: [] };
  const branding: BrandingStatus = brandingRes.ok
    ? await brandingRes.json()
    : { has_logo: false, logo_updated_at: null, brand_display_name: null, brand_accent_color: null };
  return { email, general, frameio, security, branding };
};
