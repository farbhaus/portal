import { startAuthentication, startRegistration } from "@simplewebauthn/browser";

async function postJSON<T>(url: string, body?: unknown): Promise<T> {
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    throw new Error((await res.json().catch(() => ({}))).detail ?? `Failed (${res.status})`);
  }
  return res.json();
}

/** Register a new passkey for the signed-in admin. */
export async function registerPasskey(name: string): Promise<void> {
  const optionsJSON = await postJSON<Parameters<typeof startRegistration>[0]["optionsJSON"]>(
    "/api/auth/passkeys/register/begin",
  );
  const credential = await startRegistration({ optionsJSON });
  await postJSON("/api/auth/passkeys/register/complete", { credential, name });
}

/** Passwordless login with a passkey. Throws on cancel/failure. */
export async function loginWithPasskey(): Promise<void> {
  const optionsJSON = await postJSON<Parameters<typeof startAuthentication>[0]["optionsJSON"]>(
    "/api/auth/passkeys/login/begin",
  );
  const credential = await startAuthentication({ optionsJSON });
  await postJSON("/api/auth/passkeys/login/complete", { credential });
}
