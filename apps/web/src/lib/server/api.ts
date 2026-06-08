import { env } from "$env/dynamic/private";

// Server-side base URL for the API service. In docker-compose this is the internal
// hostname; locally it defaults to the loopback Caddy port. The browser always uses
// relative /api/* (routed by Caddy), never this value.
export const INTERNAL_API_URL = env.INTERNAL_API_URL ?? "http://127.0.0.1:18080";

export async function apiFetch(
  path: string,
  cookie: string | null,
  init: RequestInit = {},
): Promise<Response> {
  const headers = new Headers(init.headers);
  if (cookie) headers.set("cookie", cookie);
  return fetch(`${INTERNAL_API_URL}${path}`, { ...init, headers });
}
