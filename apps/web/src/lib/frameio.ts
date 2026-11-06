// Frame.io folders and download sources as the admin UI handles them, plus the small API calls
// the picker makes. A FolderRef is exactly what an upload link stores as its `folder`.

import type { LinkSync } from "$lib/links";

export type PathSegment = { id: string; name: string };

export type FolderRef = {
  type?: string;
  account_id: string;
  workspace_id?: string | null;
  project_id?: string | null;
  project_name?: string | null;
  folder_id: string;
  folder_name?: string | null;
  // Breadcrumb from the project root (named after the project) down to the folder.
  path?: PathSegment[] | null;
  // Upload links only: "Sync this project's folders with FioTransfer" (Imports and Exports).
  sync?: LinkSync | null;
};

// What a download link serves. Display-only extras (path, label, names) ride along in the stored
// JSON so the list can name the source without asking Frame.io.
export type DownloadSource =
  | { type: "file"; account_id: string; file_id: string; path?: PathSegment[]; label?: string; names?: string[]; bytes?: number }
  | { type: "selection"; account_id: string; file_ids: string[]; path?: PathSegment[]; label?: string; names?: string[]; bytes?: number }
  | { type: "folder"; account_id: string; folder_id: string; recursive: boolean; path?: PathSegment[]; label?: string };

/** Folder names from the project down, for display ("Northlight / Spring / Day 3"). */
export function crumbNames(f: Partial<FolderRef> | null | undefined): string[] {
  if (!f) return [];
  const path = f.path ?? [];
  if (path.length) {
    // Older breadcrumbs name the project root "<project> (root)"; show the project name alone.
    return path.map((s, i) => (i === 0 ? s.name.replace(/ \(root\)$/, "") : s.name));
  }
  return [f.project_name, f.folder_name ?? f.folder_id].filter((x): x is string => !!x);
}

export function folderLabel(f: Partial<FolderRef> | null | undefined): string {
  return crumbNames(f).join(" / ");
}

/** A stable key for de-duplicating folders. */
export const folderKey = (f: Partial<FolderRef>) => `${f.account_id}:${f.folder_id}`;

// ── API ──────────────────────────────────────────────────────────────────────

export class FrameioApiError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message);
  }
}

export async function getJSON<T>(url: string): Promise<T> {
  const res = await fetch(url);
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new FrameioApiError(body.detail ?? `Frame.io request failed (${res.status})`, res.status);
  }
  return res.json();
}

export type Named = { id: string; name: string };
export type ProjectItem = Named & { root_folder_id: string | null };
export type FileItem = Named & { file_size: number | null; media_type?: string | null };
export type SearchHit = {
  kind: "project" | "folder";
  id: string;
  name: string;
  project_id: string | null;
  workspace_id: string | null;
  root_folder_id: string | null;
  where: string | null;
};
export type ResolvedPath = {
  account_id: string;
  workspace_id: string | null;
  project_id: string;
  project_name: string;
  root_folder_id: string | null;
  folder_id: string;
  folder_name: string;
  path: PathSegment[];
};

// Accounts and workspaces rarely change; fetch them once per page load.
const cache = new Map<string, Promise<unknown>>();
function cached<T>(key: string, load: () => Promise<T>): Promise<T> {
  if (!cache.has(key)) {
    const p = load().catch((e) => {
      cache.delete(key);
      throw e;
    });
    cache.set(key, p);
  }
  return cache.get(key) as Promise<T>;
}

export const listAccounts = () => cached("accounts", () => getJSON<Named[]>("/api/frameio/accounts"));
export const listWorkspaces = (account: string) =>
  cached(`ws:${account}`, () => getJSON<Named[]>(`/api/frameio/workspaces?account_id=${encodeURIComponent(account)}`));
export const listProjects = (account: string, workspace: string) =>
  getJSON<ProjectItem[]>(
    `/api/frameio/projects?account_id=${encodeURIComponent(account)}&workspace_id=${encodeURIComponent(workspace)}`,
  );
export const listFolders = (account: string, folder: string) =>
  getJSON<Named[]>(`/api/frameio/folders?account_id=${encodeURIComponent(account)}&folder_id=${encodeURIComponent(folder)}`);
export const listFiles = (account: string, folder: string) =>
  getJSON<FileItem[]>(`/api/frameio/files?account_id=${encodeURIComponent(account)}&folder_id=${encodeURIComponent(folder)}`);
export const search = (account: string, q: string) =>
  getJSON<SearchHit[]>(`/api/frameio/search?account_id=${encodeURIComponent(account)}&q=${encodeURIComponent(q)}`);
export const resolvePath = (account: string, folder: string) =>
  getJSON<ResolvedPath>(`/api/frameio/path?account_id=${encodeURIComponent(account)}&folder_id=${encodeURIComponent(folder)}`);

export async function createProject(account: string, workspace: string, name: string): Promise<ProjectItem> {
  const res = await fetch("/api/frameio/projects", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ account_id: account, workspace_id: workspace, name }),
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new FrameioApiError(body.detail ?? `Could not create the project (${res.status})`, res.status);
  }
  return res.json();
}

export async function createFolder(account: string, parent: string, name: string): Promise<Named> {
  const res = await fetch("/api/frameio/folders", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ account_id: account, parent_folder_id: parent, name }),
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new FrameioApiError(body.detail ?? `Could not create the folder (${res.status})`, res.status);
  }
  return res.json();
}
