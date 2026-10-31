// Links as the admin API returns them (GET /api/links, /api/upload-links, /api/download-links),
// and the plain-language status and timing the Links pages show.

import type { CoveringRule, DownloadSource, FolderRef } from "$lib/frameio";
import { crumbNames } from "$lib/frameio";
import { formatBytes } from "$lib/format";

export type LinkState = "ok" | "expired" | "revoked";

/**
 * Per-link "sync to live work", stored inside an upload link's `folder` JSON.
 *
 * `drive` is the client's drop folder: FioTransfer downloads it into the local folder named
 * `local`. `exports` is ours: the local folder named `local` is uploaded into it. Either side may
 * be left out — that lane is simply off — but a `sync` with neither is not a sync. An omitted
 * `folder_id` means "the link's own folder".
 *
 * `add_only` is the whole rule: files are added in both directions and nothing is ever deleted, so
 * Frame.io can be cleared once the local copy exists.
 */
export type LiveWorkSyncFolder = { local: string; folder_id?: string | null };
export type LiveWorkSync = {
  mode?: "add_only";
  drive?: LiveWorkSyncFolder | null;
  exports?: LiveWorkSyncFolder | null;
};

type LinkBase = {
  id: string;
  name: string;
  token: string;
  public_url: string;
  state: LinkState;
  expires_at: string | null;
  password_protected: boolean;
  verify_email: boolean;
  brand_display_name: string | null;
  brand_subtitle: string | null;
  revoked_at: string | null;
  created_at: string;
  brand_profile_id: string | null;
  notify_emails: string[] | null;
  remind_before_expiry: boolean;
};

export type UploadLink = LinkBase & {
  kind: "upload";
  // Where uploads land; also carries the optional live-work `sync`.
  folder: FolderRef | null;
  max_file_size: number | null;
  allowed_extensions: string[] | null;
  uploader_fields_required: { name: boolean; email: boolean; message: boolean };
  subfolder_template: string | null;
  // The project folder name FioTransfer reads and uses for the project's folder (null = none).
  turbosort: string | null;
};

export type DownloadLink = LinkBase & {
  kind: "download";
  source: DownloadSource & Record<string, unknown>;
  max_downloads: number | null;
  downloads_count: number;
  viewer_fields_required: { name: boolean; email: boolean };
  allow_preview: boolean;
};

export type LiveUpload = {
  id: string;
  who: string | null;
  uploaded_bytes: number | null;
  total_bytes: number | null;
  files: number;
  started_at: string | null;
  last_activity_at: string | null;
  avg_bps: number | null;
};

export type LinkActivity = {
  count: number;
  bytes?: number;
  last_at: string | null;
  last_who: string | null;
  live: LiveUpload[];
};

export type LinkItem = (UploadLink | DownloadLink) & { sync: CoveringRule[]; activity: LinkActivity };

export type UploadSessionRow = {
  id: string;
  uploader_name: string | null;
  uploader_email: string | null;
  status: string;
  files: number;
  total_bytes: number | null;
  uploaded_bytes: number | null;
  started_at: string | null;
  completed_at: string | null;
  last_activity_at: string | null;
};

export type DownloadEvent = {
  frameio_file_id: string;
  file_name: string | null;
  viewer_name: string | null;
  viewer_email: string | null;
  ip: string | null;
  user_agent: string | null;
  completed: boolean;
  started_at: string;
};

// ── time ──────────────────────────────────────────────────────────────────

const DAY = 86_400_000;

export function timeAgo(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return "—";
  const s = Math.max(0, Math.floor((now - new Date(iso).getTime()) / 1000));
  if (s < 60) return "just now";
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  if (d === 1) return "yesterday";
  if (d < 7) return `${d}d ago`;
  return shortDate(iso);
}

export function shortDate(iso: string): string {
  const d = new Date(iso);
  const sameYear = d.getFullYear() === new Date().getFullYear();
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short", year: sameYear ? undefined : "numeric" });
}

export function whenFull(iso: string | null | undefined): string {
  if (!iso) return "—";
  return new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** "in 6 days", "tomorrow", "in 5h", "Never" — or the date it expired. */
export function expiresText(link: Pick<LinkBase, "expires_at" | "state">, now = Date.now()): string {
  if (link.state === "revoked") return "—";
  if (!link.expires_at) return "Never";
  const t = new Date(link.expires_at).getTime();
  if (t <= now) return shortDate(link.expires_at);
  const ms = t - now;
  if (ms < 3_600_000) return `in ${Math.max(1, Math.round(ms / 60_000))}m`;
  if (ms < DAY) return `in ${Math.round(ms / 3_600_000)}h`;
  const days = Math.round(ms / DAY);
  if (days === 1) return "tomorrow";
  if (days < 60) return `in ${days} days`;
  return shortDate(link.expires_at);
}

export function fmtDuration(seconds: number): string {
  if (!isFinite(seconds) || seconds <= 0) return "—";
  const s = Math.round(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h) return `${h}h ${String(m).padStart(2, "0")}m`;
  if (m) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  return `${s}s`;
}

export const fmtSpeed = (bps: number | null | undefined) => (bps && bps > 0 ? `${formatBytes(bps)}/s` : "—");

/** Progress of an upload in flight: fraction done, and time left at its average speed. */
export function liveProgress(u: LiveUpload): { pct: number | null; eta: string | null } {
  const done = u.uploaded_bytes ?? 0;
  const total = u.total_bytes ?? 0;
  const pct = total > 0 ? Math.min(100, (100 * done) / total) : null;
  const eta = total > done && u.avg_bps ? `${fmtDuration((total - done) / u.avg_bps)} left` : null;
  return { pct, eta };
}

// ── naming ────────────────────────────────────────────────────────────────

/** Where a link points, as breadcrumb names. */
export function linkPath(l: UploadLink | DownloadLink): string[] {
  if (l.kind === "upload") return crumbNames(l.folder);
  const p = (l.source.path as { id: string; name: string }[] | undefined) ?? [];
  return crumbNames({ path: p, account_id: "", folder_id: "" });
}

/** "24 files", "Folder · with subfolders", "A001_C003.mov". */
export function sourceSummary(src: DownloadLink["source"]): string {
  const bytes = typeof src.bytes === "number" && src.bytes > 0 ? ` · ${formatBytes(src.bytes)}` : "";
  if (src.type === "file") return `${(src.names as string[] | undefined)?.[0] ?? (src.label as string | undefined) ?? "1 file"}${bytes}`;
  if (src.type === "selection") return `${(src.file_ids as string[]).length} files${bytes}`;
  if (src.type === "folder") return src.recursive ? "Whole folder, with subfolders" : "Whole folder";
  return "—";
}

export type Status = { tone: "live" | "ok" | "muted" | "warn" | "bad"; text: string };

export function linkStatus(l: LinkItem): Status {
  if (l.state === "revoked") return { tone: "bad", text: "Revoked" };
  if (l.state === "expired") return { tone: "muted", text: "Expired" };
  const live = l.activity.live[0];
  if (live) {
    const { eta } = liveProgress(live);
    return { tone: "live", text: eta ? `Receiving · ${eta}` : "Receiving" };
  }
  if (l.kind === "download" && l.max_downloads && l.downloads_count >= l.max_downloads) {
    return { tone: "warn", text: "Download limit reached" };
  }
  return { tone: "ok", text: "Active" };
}

export const kindNoun = (k: "upload" | "download") => (k === "upload" ? "Receive" : "Send");
