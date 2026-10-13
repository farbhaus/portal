import type { PageLoad } from "./$types";
import type { Destination } from "../../destinations/+page";

export const load: PageLoad = async ({ fetch }) => {
  const [destRes, presetRes] = await Promise.all([
    fetch("/api/destinations"),
    fetch("/api/upload-links/extension-preset"),
  ]);
  const destinations: Destination[] = destRes.ok ? await destRes.json() : [];
  const extensionPreset: string[] = presetRes.ok ? await presetRes.json() : [];
  return { destinations, extensionPreset };
};
