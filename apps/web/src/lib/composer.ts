// What every link composer page loads besides the link itself: recent folders (from existing
// upload links), the page branding for the preview, whether email is set up (for "Email to
// client"), and for FioTransfer the Imports/Exports names from Settings and the project folders
// links already use.
import type { Branding, Preset } from "$lib/components";
import type { FolderRef } from "$lib/frameio";
import { DEFAULT_LANE_NAMES, type LaneNames, type UploadLink } from "$lib/links";
import { recentFolders } from "$lib/recents";

type Fetch = typeof fetch;

export async function loadComposerContext(fetch: Fetch) {
  const [upRes, brandRes, emailRes, presetsRes, laneRes] = await Promise.all([
    fetch("/api/upload-links"),
    fetch("/api/settings/branding"),
    fetch("/api/settings/email"),
    fetch("/api/link-presets"),
    fetch("/api/settings/fiotransfer"),
  ]);
  const uploads: UploadLink[] = upRes.ok ? await upRes.json() : [];
  const b = brandRes.ok ? await brandRes.json() : {};
  const email = emailRes.ok ? await emailRes.json() : {};
  const branding: Branding = {
    name: b.brand_display_name ?? null,
    accent: b.brand_accent_color ?? null,
    logo_url: b.has_logo ? `/api/branding/logo?v=${b.logo_updated_at ? Date.parse(b.logo_updated_at) : 0}` : null,
  };
  const presets: Preset[] = presetsRes.ok ? await presetsRes.json() : [];
  const lanes = laneRes.ok ? await laneRes.json() : {};
  const laneNames: LaneNames = {
    imports: lanes.imports_folder || DEFAULT_LANE_NAMES.imports,
    exports: lanes.exports_folder || DEFAULT_LANE_NAMES.exports,
  };
  const projectFolders = [...new Set(uploads.map((u) => u.turbosort?.trim()).filter((v): v is string => !!v))].sort((a, b) =>
    a.localeCompare(b),
  );
  return {
    uploads,
    branding,
    presets,
    defaultNotify: (email.notify_email as string | undefined) ?? null,
    emailConfigured: email.configured !== false,
    laneNames,
    projectFolders,
    // Recents read localStorage, so they're computed in the browser by the page.
    recentsFrom: uploads as { kind: string; folder?: FolderRef | null; created_at: string }[],
  };
}

export { recentFolders };
