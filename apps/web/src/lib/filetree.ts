// Turn a flat download listing into a navigable folder tree.
//
// The API returns every file of a link in one list, each carrying `path` — its folder relative to
// the link's source root ("" = directly in the root, "" for every file of a flat/non-recursive
// source). Rather than fetch a folder at a time, the recipient page derives the tree from that
// listing and browses it client-side.

import type { DownloadFile } from "$lib/download";

export type FolderEntry = {
  name: string; // segment name, as shown in the row
  path: string; // full path of the folder, relative to the source root
  fileCount: number; // files anywhere below it (the tree has no empty folders to count)
  size: number | null; // total bytes below it, or null if any file's size is unknown
};

/** Strip stray leading/trailing slashes so "" is the one spelling of "the root". */
const norm = (path: string): string => path.replace(/^\/+|\/+$/g, "");

/** Is `path` the folder `cwd` itself or one below it? */
function under(path: string, cwd: string): boolean {
  return cwd === "" || path === cwd || path.startsWith(`${cwd}/`);
}

/** `path` re-expressed relative to `cwd` ("" when they are the same folder). */
function relative(path: string, cwd: string): string {
  if (cwd === "") return path;
  return path === cwd ? "" : path.slice(cwd.length + 1);
}

const byName = (a: { name: string }, b: { name: string }) =>
  a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });

/** Does this listing have any folder structure worth navigating? */
export function hasFolders(files: DownloadFile[]): boolean {
  return files.some((f) => norm(f.path) !== "");
}

/** The immediate subfolders of `cwd`, each summarising everything beneath it. */
export function childFolders(files: DownloadFile[], cwd: string): FolderEntry[] {
  const folders = new Map<string, FolderEntry>();
  for (const file of files) {
    const path = norm(file.path);
    if (!under(path, cwd)) continue;
    const rest = relative(path, cwd);
    if (!rest) continue; // file sits directly in cwd, not in a subfolder
    const name = rest.split("/")[0];
    const full = cwd ? `${cwd}/${name}` : name;
    const entry = folders.get(full) ?? { name, path: full, fileCount: 0, size: 0 };
    entry.fileCount += 1;
    entry.size = entry.size === null || file.size === null ? null : entry.size + file.size;
    folders.set(full, entry);
  }
  return [...folders.values()].sort(byName);
}

/** The files sitting directly in `cwd` (not in its subfolders), sorted by name. */
export function filesIn(files: DownloadFile[], cwd: string): DownloadFile[] {
  return files.filter((f) => norm(f.path) === cwd).sort(byName);
}

/**
 * Every file at or below `cwd`, with `path` rewritten relative to it — so "download all" from
 * inside a subfolder recreates that subfolder's tree, not the whole link's.
 */
export function subtree(files: DownloadFile[], cwd: string): DownloadFile[] {
  const out: DownloadFile[] = [];
  for (const file of files) {
    const path = norm(file.path);
    if (!under(path, cwd)) continue;
    out.push(cwd ? { ...file, path: relative(path, cwd) } : file);
  }
  return out;
}

/**
 * The in-zip path of every file, in listing order: its folder plus a name made unique within that
 * folder. Frame.io allows two files to share a name in one folder, and a ZIP holding two identical
 * entry names loses one of them silently on extraction — so collisions become "clip (2).mov".
 */
export function zipPaths(files: DownloadFile[]): string[] {
  const seen = new Set<string>();
  return files.map((file) => {
    const dir = norm(file.path);
    const dot = file.name.lastIndexOf(".");
    // Treat a leading dot as part of the name, not an extension (".DS_Store" has no stem).
    const [stem, ext] = dot > 0 ? [file.name.slice(0, dot), file.name.slice(dot)] : [file.name, ""];
    let name = file.name;
    for (let n = 2; seen.has(`${dir}/${name}`.toLowerCase()); n++) name = `${stem} (${n})${ext}`;
    seen.add(`${dir}/${name}`.toLowerCase());
    return dir ? `${dir}/${name}` : name;
  });
}

/** Breadcrumb segments for `cwd`, starting with the root (named by the caller). */
export function crumbs(cwd: string, rootName: string): { name: string; path: string }[] {
  const segments = cwd ? cwd.split("/") : [];
  let acc = "";
  return [
    { name: rootName, path: "" },
    ...segments.map((name) => {
      acc = acc ? `${acc}/${name}` : name;
      return { name, path: acc };
    }),
  ];
}
