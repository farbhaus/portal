// Remembers the uploader's name/email in localStorage so returning uploaders get
// prefilled fields. Best-effort only: storage may be unavailable (private mode),
// and entries expire 30 days after last use.

const KEY = "portal:uploader";
const MAX_AGE_MS = 30 * 24 * 60 * 60 * 1000;

export interface UploaderPrefs {
  name: string;
  email: string;
}

export function loadUploaderPrefs(): UploaderPrefs | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const data = JSON.parse(raw);
    if (typeof data?.savedAt !== "number" || Date.now() - data.savedAt > MAX_AGE_MS) {
      localStorage.removeItem(KEY);
      return null;
    }
    return {
      name: typeof data.name === "string" ? data.name : "",
      email: typeof data.email === "string" ? data.email : "",
    };
  } catch {
    return null;
  }
}

export function saveUploaderPrefs(name: string, email: string): void {
  try {
    localStorage.setItem(KEY, JSON.stringify({ name, email, savedAt: Date.now() }));
  } catch {
    // Storage unavailable — remembering is a convenience, not a requirement.
  }
}
