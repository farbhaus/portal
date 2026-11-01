import type { PageLoad } from "./$types";
import { loadComposerContext } from "$lib/composer";
import type { FolderRef } from "$lib/frameio";

export const load: PageLoad = async ({ fetch, url }) => {
  const ctx = await loadComposerContext(fetch);
  const type = url.searchParams.get("type");
  // "Receive into this folder" from elsewhere (Files, ⌘K): ?account_id=&folder_id=
  let folder: FolderRef | null = null;
  const account = url.searchParams.get("account_id");
  const folderId = url.searchParams.get("folder_id");
  if (account && folderId) {
    const res = await fetch(`/api/frameio/path?account_id=${encodeURIComponent(account)}&folder_id=${encodeURIComponent(folderId)}`);
    if (res.ok) folder = { type: "frameio", ...(await res.json()) };
  }
  return {
    ...ctx,
    kind: type === "download" ? ("download" as const) : ("upload" as const),
    pick: url.searchParams.get("pick") === "1",
    folder,
  };
};
