<script lang="ts" module>
  export type Branding = { name: string | null; accent: string | null; logo_url: string | null };
  export type Preset = { id: string; name: string; kind: "upload" | "download"; settings: Record<string, unknown> };

  // File-type groups for upload links. "Anything" stores no restriction at all.
  export const TYPE_GROUPS = {
    video: { label: "Video", ext: ["r3d", "braw", "ari", "arx", "mxf", "mov", "mp4", "mts", "m4v", "avi", "prores"] },
    audio: { label: "Audio", ext: ["wav", "aif", "aiff", "mp3", "bwf"] },
    stills: { label: "Stills", ext: ["dng", "cdng", "exr", "dpx", "tiff", "tif", "png", "jpg", "jpeg"] },
    docs: { label: "Documents", ext: ["pdf", "edl", "xml", "ale", "aaf", "fcpxml", "otio", "txt", "csv", "zip"] },
  } as const;
  export type TypeGroup = keyof typeof TYPE_GROUPS;
</script>

<script lang="ts">
  // Create or edit a link — both kinds — on one screen. The minimum is three decisions: receive or
  // send, which folder (or files), and how long. Everything else has a default and waits under
  // More options. The right column shows the client's page as you build it.
  import { goto, invalidateAll } from "$app/navigation";
  import { onMount, untrack } from "svelte";
  import FolderField from "./FolderField.svelte";
  import FrameioPicker, { type SourcePick } from "./FrameioPicker.svelte";
  import Icon from "./Icon.svelte";
  import Switch from "./Switch.svelte";
  import ConfirmButton from "./ConfirmButton.svelte";
  import { createFolder, crumbNames, listFolders, resolvePath, type DownloadSource, type FolderRef, type Named, type PathSegment } from "$lib/frameio";
  import { DEFAULT_LANE_NAMES, diskName, kindNoun, sourceSummary, type DownloadLink, type LaneNames, type LinkSync, type UploadLink } from "$lib/links";
  import { copyWithToast, errorDetail, toast } from "$lib/toast.svelte";
  import { expiryDate, formatBytes, accentText } from "$lib/format";
  import { UPLOAD_TOKENS, templateAsks } from "$lib/tokens";
  import { rememberFolder } from "$lib/recents";

  type Kind = "upload" | "download";
  type Expiry = "keep" | "1" | "7" | "30" | "never" | "custom";

  let {
    mode = "create",
    kind: initialKind = "upload",
    link = null,
    recents = [],
    branding,
    initialFolder = null,
    openSourcePicker = false,
    emailConfigured = true,
    presets = [],
    defaultNotify = null,
    laneNames = DEFAULT_LANE_NAMES,
    projectFolders = [],
  }: {
    mode?: "create" | "edit";
    kind?: Kind;
    link?: UploadLink | DownloadLink | null;
    recents?: FolderRef[];
    branding: Branding;
    initialFolder?: FolderRef | null;
    openSourcePicker?: boolean;
    emailConfigured?: boolean;
    presets?: Preset[];
    defaultNotify?: string | null;
    /** The Imports and Exports folder names from Settings. */
    laneNames?: LaneNames;
    /** Project folders other links already use, offered for this one. */
    projectFolders?: string[];
  } = $props();

  const editing = $derived(mode === "edit");
  const up = $derived(link?.kind === "upload" ? (link as UploadLink) : null);
  const down = $derived(link?.kind === "download" ? (link as DownloadLink) : null);

  // ── form state ──────────────────────────────────────────────────────────
  let kind = $state<Kind>(untrack(() => link?.kind ?? initialKind));
  let folder = $state<FolderRef | null>(null);
  let source = $state<SourcePick | null>(null);
  let name = $state("");
  let nameTouched = $state(false);
  let title = $state("");
  let subtitle = $state("");
  let expiry = $state<Expiry>("7");
  let customDate = $state("");
  let pwMode = $state<"none" | "keep" | "set" | "clear">("none");
  let password = $state("");
  let otp = $state(false);
  let askName = $state(false);
  let askEmail = $state(false);
  let askMessage = $state(false);
  let askProject = $state(false);
  let types = $state<Set<TypeGroup | "any">>(new Set(["any"]));
  let customExt = $state<string[]>([]);
  let maxSizeGb = $state("");
  let template = $state("");
  // "Sync this project's folders with FioTransfer": a project folder on the user's machine
  // holding up to two one-way folders. Imports bring what the client sends down; Exports take what
  // the user puts in theirs up to a Frame.io folder. Each lane is a switch of its own.
  let syncOn = $state(false);
  let turbosort = $state("");
  let turbosortTouched = $state(false);
  let importsOn = $state(true);
  // Where Imports come down from; null is the folder the link receives into.
  let importsFolder = $state<FolderRef | null>(null);
  // The lanes' names on disk as saved: kept while a lane's Frame.io folder isn't known yet.
  let savedImportsLocal = $state("");
  let exportsOn = $state(true);
  let exportsFolder = $state<FolderRef | null>(null);
  let savedExportsLocal = $state("");
  let sendLink = $state(true);
  // The job folder the lanes sit in, and its folders: where Exports is looked for by name.
  let job = $state<{ ref: FolderRef; name: string } | null>(null);
  let jobChildren = $state<Named[] | null>(null);
  let syncError = $state<string | null>(null);
  let maxDownloads = $state("");
  let preview = $state(true);
  let preset = $state("");
  let notifyEmails = $state("");
  let remind = $state(false);
  let savedPresets = $state<Preset[]>(untrack(() => [...presets]));
  let presetNaming = $state(false);
  let presetName = $state("");
  let sourceOpen = $state(false);

  let saving = $state(false);
  let error = $state<string | null>(null);
  let created = $state<(UploadLink | DownloadLink) | null>(null);
  let copied = $state(false);
  let emailOpen = $state(false);
  let emailTo = $state("");
  let emailNote = $state("");
  let emailSending = $state(false);

  // Seed from the link being edited (or sensible defaults when creating).
  function seed() {
    error = null;
    if (link) {
      name = link.name;
      nameTouched = true;
      title = link.brand_display_name ?? "";
      subtitle = link.brand_subtitle ?? "";
      expiry = link.expires_at ? "keep" : "never";
      pwMode = link.password_protected ? "keep" : "none";
      password = "";
      otp = link.verify_email;
      notifyEmails = (link.notify_emails ?? []).join(", ");
      remind = link.remind_before_expiry;
      if (link.kind === "upload") {
        folder = link.folder;
        // What the template asks for shows as asked; only the rest are the link's own choice.
        const needs = templateAsks(link.subfolder_template);
        askName = link.uploader_fields_required.name && !needs.name;
        askEmail = link.uploader_fields_required.email && !link.verify_email;
        askMessage = link.uploader_fields_required.message;
        askProject = link.uploader_fields_required.project && !needs.project;
        const ext = link.allowed_extensions;
        if (!ext?.length) {
          types = new Set(["any"]);
          customExt = [];
        } else {
          const groups = (Object.keys(TYPE_GROUPS) as TypeGroup[]).filter((g) => (TYPE_GROUPS[g].ext as readonly string[]).some((e) => ext.includes(e)));
          const known = new Set<string>(groups.flatMap((g) => [...TYPE_GROUPS[g].ext]));
          types = new Set(groups.length ? groups : []);
          customExt = ext.filter((e) => !known.has(e));
          if (!groups.length && !customExt.length) types = new Set(["any"]);
        }
        maxSizeGb = link.max_file_size ? String(+(link.max_file_size / 1e9).toFixed(2)) : "";
        template = link.subfolder_template ?? "";
        const s = link.folder?.sync ?? null;
        const account = link.folder?.account_id ?? "";
        syncOn = !!s;
        turbosort = link.turbosort ?? "";
        turbosortTouched = !!link.turbosort;
        // An existing link shows its lanes as saved: a lane it doesn't have stays off, so saving
        // never starts moving files the link deliberately didn't move. Only ids are stored; their
        // breadcrumbs are fetched once the composer is up.
        importsOn = s ? !!s.drive : true;
        importsFolder = s?.drive?.folder_id ? { account_id: account, folder_id: s.drive.folder_id } : null;
        savedImportsLocal = s?.drive?.local ?? "";
        exportsOn = s ? !!s.exports : true;
        exportsFolder = s?.exports?.folder_id ? { account_id: account, folder_id: s.exports.folder_id } : null;
        savedExportsLocal = s?.exports?.local ?? "";
        sendLink = link.send_link ?? true;
      } else {
        const s = link.source;
        source = { source: s as DownloadSource, label: (s.label as string) ?? sourceSummary(s), path: (s.path as FolderRef["path"]) ?? [], count: null, bytes: (s.bytes as number) ?? null };
        askName = link.viewer_fields_required.name;
        askEmail = link.viewer_fields_required.email && !link.verify_email;
        maxDownloads = link.max_downloads ? String(link.max_downloads) : "";
        preview = link.allow_preview;
      }
    } else {
      folder = initialFolder;
      expiry = "7";
    }
  }
  seed();

  onMount(() => {
    if (openSourcePicker && kind === "download" && !source) sourceOpen = true;
  });

  // ── derived ─────────────────────────────────────────────────────────────
  // The page brand: the app-wide one from Settings.
  const accent = $derived(branding.accent || "#f59e0b");
  const brandName = $derived(branding.name || "Portal");
  const logoUrl = $derived(branding.logo_url);

  function endOfDay(days: number): Date {
    const d = new Date();
    d.setDate(d.getDate() + days);
    d.setHours(23, 59, 0, 0);
    return d;
  }
  const expiresAt = $derived.by((): Date | null | "keep" => {
    if (expiry === "keep") return "keep";
    if (expiry === "never") return null;
    if (expiry === "custom") return customDate ? new Date(`${customDate}T23:59:00`) : null;
    return endOfDay(Number(expiry));
  });
  // The chip that keeps an edited link's end date: the date itself, with the year when it isn't this one.
  function savedDay(at: string): string {
    const d = new Date(at);
    const year = d.getFullYear() !== new Date().getFullYear() ? { year: "numeric" as const } : {};
    return d.toLocaleDateString(undefined, { day: "numeric", month: "short", ...year });
  }
  // What the client page says about the end: "Expires on …", or nothing for a link that stays open.
  const clientExpiry = $derived.by(() => {
    const end = expiresAt === "keep" ? link?.expires_at : expiresAt;
    return end ? `Expires on ${expiryDate(end)}` : "";
  });

  const allowedExtensions = $derived.by((): string[] | null => {
    if (types.has("any")) return customExt.length ? null : null;
    const ext = [...types].flatMap((g) => [...TYPE_GROUPS[g as TypeGroup].ext]);
    const all = [...ext, ...customExt];
    return all.length ? all : null;
  });
  const typesLabel = $derived(
    types.has("any") || !allowedExtensions
      ? "Any file type"
      : [...[...types].map((g) => TYPE_GROUPS[g as TypeGroup].label), ...(customExt.length ? [customExt.map((e) => `.${e}`).join(" ")] : [])].join(", "),
  );

  const tplExample = $derived.by(() => {
    const d = new Date();
    const pad = (n: number) => String(n).padStart(2, "0");
    const vals: Record<string, string> = {
      date: `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`,
      year: String(d.getFullYear()),
      month: pad(d.getMonth() + 1),
      day: pad(d.getDate()),
      time: `${pad(d.getHours())}h${pad(d.getMinutes())}`,
      uploader_name: "Ana Ribeiro",
      project: "Harbour Lights",
    };
    const parts = template.trim() ? template.replace(/\{(\w+)\}/g, (m, k) => vals[k] ?? m).split("/").map((x) => x.trim()).filter(Boolean) : [];
    // Synced into its job folder, the link receives into the job's Imports folder (`receivePlan`).
    return [crumbNames(folder).at(-1) ?? "Folder", ...(receivePlan ? [receivePlan.name] : []), ...parts];
  });
  // The fields the Organize template fills folders from: asked whatever their chips say.
  const tplAsks = $derived(kind === "upload" ? templateAsks(template) : { name: false, project: false });


  // ── interactions ────────────────────────────────────────────────────────
  function setKind(k: Kind) {
    if (editing || k === kind) return;
    kind = k;
    if (!nameTouched) name = k === "upload" ? "" : (source?.label ?? "");
    if (k === "download" && !source) sourceOpen = true;
  }

  function onFolder(f: FolderRef) {
    rememberFolder(f);
  }

  function onSource(p: SourcePick) {
    source = p;
    if (!nameTouched) name = p.label;
  }

  function toggleType(g: TypeGroup | "any") {
    const next = new Set(types);
    if (g === "any") {
      types = new Set(["any"]);
      customExt = [];
      return;
    }
    next.delete("any");
    if (next.has(g)) next.delete(g);
    else next.add(g);
    if (!next.size && !customExt.length) next.add("any");
    types = next;
  }

  function insertToken(tok: string) {
    const t = `{${tok}}`;
    if (template.includes(t)) {
      template = template.replace(`${t}/`, "").replace(`/${t}`, "").replace(t, "");
      return;
    }
    template = template && !template.endsWith("/") ? `${template}/${t}` : `${template}${t}`;
  }

  const WORDS = ["amber", "slate", "grain", "velvet", "reel", "frame", "focus", "prism", "dolly", "lens", "chroma", "tide", "atlas", "ember", "cedar", "orbit"];
  function passphrase(): string {
    const pick = () => WORDS[Math.floor(Math.random() * WORDS.length)];
    return `${pick()}-${pick()}-${10 + Math.floor(Math.random() * 90)}`;
  }

  // ── the project folder and its Imports/Exports (sync) ──────────────────
  const folderKey = $derived(kind === "upload" && syncOn && folder ? `${folder.account_id}:${folder.folder_id}` : "");
  let folderSeq = 0;

  $effect(() => {
    const key = folderKey;
    untrack(() => void loadJob(key));
  });

  /** Breadcrumb ids of a folder, root first, ending at the folder itself. */
  const pathIds = (f: Partial<FolderRef> | null | undefined) => (f?.path ?? []).map((s) => s.id);
  /** macOS and Windows ignore case: RUSHES and rushes are one folder. */
  const sameName = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();
  /**
   * Where Imports come down from: the folder they name, else the one the link receives into. None
   * while the link is still to receive into its job folder's Imports folder (`receivePlan`).
   */
  const importsFrom = (): FolderRef | null => (importsOn && !receivePlan ? (importsFolder ?? folder) : null);

  /**
   * The job folder the lanes sit in, by the rule the server names an unnamed project folder by:
   * the folder above the link's folder when that is the Imports folder itself (named like the
   * Imports setting, as `ACME / 1_DRIVE` is), else the first folder below the project (a job
   * folder), else the project's root, which goes by the project's name.
   */
  function jobOf(f: FolderRef, path: PathSegment[]): { ref: FolderRef; name: string } | null {
    if (!path.length) return null;
    const i = path.length === 1 ? 0 : sameName(path[path.length - 1].name, laneNames.imports) ? path.length - 2 : 1;
    const seg = path[i];
    const name = i === 0 ? f.project_name || seg.name.replace(/ \(root\)$/, "") : seg.name;
    return { ref: { ...f, folder_id: seg.id, folder_name: name, path: path.slice(0, i + 1), sync: null }, name };
  }

  /** Find the job folder and its folders, name the project folder after it, and look for Exports. */
  async function loadJob(key: string) {
    const f = folder;
    if (!key || !f) {
      job = null;
      jobChildren = null;
      syncError = null;
      return;
    }
    const my = ++folderSeq;
    syncError = null;
    try {
      const resolved = f.path?.length ? null : await resolvePath(f.account_id, f.folder_id);
      if (my !== folderSeq) return;
      const path = resolved ? resolved.path : (f.path ?? []);
      const ref = resolved ? { ...f, project_id: f.project_id ?? resolved.project_id, project_name: f.project_name ?? resolved.project_name } : f;
      const j = jobOf(ref, path);
      job = j;
      jobChildren = null;
      if (j && !turbosortTouched) turbosort = diskName(j.name);
      if (!j) return;
      const kids = await listFolders(f.account_id, j.ref.folder_id);
      if (my !== folderSeq) return;
      jobChildren = [...kids].sort((a, b) => a.name.localeCompare(b.name));
    } catch (e) {
      if (my !== folderSeq) return;
      syncError = e instanceof Error ? e.message : "Could not read this folder from Frame.io";
    }
  }

  /** A folder of the job folder, as a FolderRef the pickers and the payload take. */
  const childRef = (j: { ref: FolderRef }, c: Named): FolderRef => ({ ...j.ref, folder_id: c.id, folder_name: c.name, path: [...(j.ref.path ?? []), { id: c.id, name: c.name }] });
  /** Whether a folder of the job folder can take Exports: the job folder isn't the Imports one. */
  const jobTakesExports = () => !!job && job.ref.folder_id !== importsFrom()?.folder_id;

  /**
   * The job folder's own Imports and Exports folders, named as in Settings, so they match the
   * folders on disk. With the sync on, a link picked into its job folder
   * receives into the Imports one, and Exports go up to the Exports one. One Frame.io doesn't have
   * yet is made when the link is saved, never before. A synced link keeps where it receives unless
   * its folder changes.
   */
  type Plan = { name: string; existing: Named | null };
  function planFor(name: string): Plan | null {
    const n = diskName(name);
    if (!n || !jobChildren) return null;
    const hit = jobChildren.find((c) => c.name === n) ?? jobChildren.find((c) => sameName(c.name, n)) ?? null;
    return { name: hit?.name ?? n, existing: hit };
  }
  const receivePlan = $derived.by((): Plan | null => {
    if (kind !== "upload" || !syncOn || !importsOn || importsFolder || !folder || !job) return null;
    if (folder.folder_id !== job.ref.folder_id) return null;
    if (editing && up?.folder?.sync && folder.folder_id === up.folder.folder_id) return null;
    return planFor(laneNames.imports);
  });
  const exportsPlan = $derived.by((): Plan | null => {
    if (kind !== "upload" || !syncOn || !exportsOn || exportsFolder || !job || !jobTakesExports()) return null;
    return planFor(laneNames.exports);
  });

  /** The job folder's folder called `name`, made when missing (looked up again first: never twice). */
  async function ensureChild(j: { ref: FolderRef }, name: string): Promise<Named> {
    const kids = await listFolders(j.ref.account_id, j.ref.folder_id);
    const hit = kids.find((c) => sameName(c.name, name));
    if (hit) return hit;
    const made = await createFolder(j.ref.account_id, j.ref.folder_id, name);
    jobChildren = [...(jobChildren ?? []), made].sort((a, b) => a.name.localeCompare(b.name));
    return made;
  }

  /** On save: the link receives into the planned Imports folder, Exports go up to the other. */
  async function realizePlans() {
    const j = job;
    const rp = receivePlan;
    const ep = exportsPlan;
    if (!j) return;
    if (rp) folder = childRef(j, rp.existing ?? (await ensureChild(j, rp.name)));
    if (ep) exportsFolder = childRef(j, ep.existing ?? (await ensureChild(j, ep.name)));
  }

  /** A folder's own name, when it is known (a saved one is only an id until its breadcrumb loads). */
  const nameOf = (f: FolderRef | null | undefined): string | null =>
    f?.path?.length ? (crumbNames(f).at(-1) ?? null) : (f?.folder_name ?? null);

  /**
   * Each lane's folder on disk is named like its Frame.io folder, so nothing is named twice: Imports
   * like the folder they come down from, Exports like the one they go up to. Until that folder's
   * name is known, the saved name (else the Settings one) stands.
   */
  const importsLocal = $derived.by(() => {
    const name = importsFolder ? nameOf(importsFolder) : receivePlan ? receivePlan.name : nameOf(folder);
    return diskName(name ?? "") || savedImportsLocal || laneNames.imports;
  });
  const exportsLocal = $derived.by(() => {
    const name = exportsFolder ? nameOf(exportsFolder) : exportsPlan?.name;
    return diskName(name ?? "") || savedExportsLocal || laneNames.exports;
  });

  /**
   * Imports come down from a folder the client's uploads don't land in (not the link's folder,
   * nor one holding it): nothing the client sends through this link would come down.
   */
  const importsMissUploads = $derived(
    kind === "upload" && syncOn && importsOn && !!importsFolder && !!folder &&
      importsFolder.folder_id !== folder.folder_id && !pathIds(folder).includes(importsFolder.folder_id),
  );
  /** Receive into the Imports folder instead, so what the client sends lands where Imports come from. */
  function receiveIntoImports() {
    const f = importsFolder;
    if (!f) return;
    folder = { type: "frameio", ...f, sync: null };
    importsFolder = null;
  }
  // A saved lane folder is only an id: fetch its breadcrumb once, to show where it is.
  $effect(() => {
    const r = importsFolder;
    if (!r || r.path?.length) return;
    untrack(() => {
      void resolvePath(r.account_id, r.folder_id)
        .then((p) => {
          if (importsFolder?.folder_id === r.folder_id) importsFolder = { type: "frameio", ...p };
        })
        .catch(() => {});
    });
  });
  $effect(() => {
    const r = exportsFolder;
    if (!r || r.path?.length) return;
    untrack(() => {
      void resolvePath(r.account_id, r.folder_id)
        .then((p) => {
          if (exportsFolder?.folder_id === r.folder_id) exportsFolder = { type: "frameio", ...p };
        })
        .catch(() => {});
    });
  });

  /**
   * What stops the lanes from saving. Exports go up somewhere Imports don't come down from, and
   * not inside it: FioTransfer brings that folder down whole, so what goes up would come back.
   */
  const exportsMissing = $derived(kind === "upload" && syncOn && exportsOn && !exportsFolder && !exportsPlan);
  const laneIssue = $derived.by((): string | null => {
    if (kind !== "upload" || !syncOn) return null;
    if (!importsOn && !exportsOn) return "Turn on Imports or Exports, or stop syncing this link.";
    const e = exportsOn ? exportsFolder : null;
    const from = importsFrom();
    if (e && from && e.folder_id === from.folder_id) return "Exports can't go up to the folder Imports come down from.";
    if (e && from && pathIds(e).slice(0, -1).includes(from.folder_id)) return "Exports can't be inside the folder Imports come down from.";
    if (importsOn && exportsOn && (exportsFolder || exportsPlan) && sameName(importsLocal, exportsLocal)) return "Imports and Exports need folders with different names.";
    return null;
  });

  // ── presets ─────────────────────────────────────────────────────────────
  // What a preset remembers: every option except where files go and the link's own name/title.
  // A picked (or kept) date is remembered as so many days from today.
  function presetExpiry(): string {
    if (expiry !== "custom" && expiry !== "keep") return expiry;
    const end = expiry === "keep" ? (link?.expires_at ? new Date(link.expires_at) : null) : (expiresAt as Date | null);
    if (!end) return expiry === "keep" ? "never" : "7";
    return String(Math.max(1, Math.round((end.getTime() - endOfDay(0).getTime()) / 86_400_000)));
  }
  function snapshot(): Record<string, unknown> {
    return {
      expiry: presetExpiry(),
      otp,
      password: pwMode === "set" || pwMode === "keep",
      ask: { name: askName, email: askEmail, message: askMessage, project: askProject },
      subtitle: subtitle.trim(),
      notify_emails: notifyEmails.trim(),
      remind,
      ...(kind === "upload"
        ? { types: [...types], custom_ext: customExt, max_size_gb: maxSizeGb, template }
        : { max_downloads: maxDownloads, allow_preview: preview }),
    };
  }

  function applyPreset(id: string) {
    preset = "";
    if (id === "__save") {
      presetNaming = true;
      presetName = "";
      return;
    }
    const p = savedPresets.find((x) => x.id === id);
    if (!p) return;
    const st = p.settings as Record<string, any>;
    if (typeof st.expiry === "string") {
      if (["1", "7", "30", "never"].includes(st.expiry)) expiry = st.expiry as Expiry;
      else if (/^\d+$/.test(st.expiry)) {
        const d = endOfDay(Number(st.expiry));
        customDate = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
        expiry = "custom";
      }
    }
    otp = !!st.otp;
    if (st.password) {
      if (pwMode !== "keep") {
        pwMode = "set";
        if (!password) password = passphrase();
      }
    } else if (pwMode === "set") {
      pwMode = link?.password_protected ? "keep" : "none";
    }
    askName = !!st.ask?.name;
    askEmail = !!st.ask?.email;
    askMessage = !!st.ask?.message;
    askProject = !!st.ask?.project;
    if (typeof st.subtitle === "string" && st.subtitle) subtitle = st.subtitle;
    notifyEmails = typeof st.notify_emails === "string" ? st.notify_emails : "";
    remind = !!st.remind;
    if (kind === "upload") {
      if (Array.isArray(st.types) && st.types.length) types = new Set(st.types as (TypeGroup | "any")[]);
      customExt = Array.isArray(st.custom_ext) ? (st.custom_ext as string[]) : [];
      maxSizeGb = typeof st.max_size_gb === "string" ? st.max_size_gb : "";
      template = typeof st.template === "string" ? st.template : "";
    } else {
      maxDownloads = typeof st.max_downloads === "string" ? st.max_downloads : "";
      preview = st.allow_preview !== false;
    }
    toast(`Applied “${p.name}”`);
  }

  async function savePreset() {
    const n = presetName.trim();
    if (!n) return;
    const res = await fetch("/api/link-presets", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: n, kind, settings: snapshot() }),
    });
    if (!res.ok) return toast(await errorDetail(res, "Could not save the preset"), { kind: "error" });
    savedPresets = [...savedPresets, await res.json()];
    presetNaming = false;
    toast(`Saved “${n}”. Manage presets in Settings.`);
  }

  const kindPresets = $derived(savedPresets.filter((p) => p.kind === kind));

  // ── save ────────────────────────────────────────────────────────────────
  /** `folder.sync` as the API stores it, or null when the switch is off. A lane that's off is absent. */
  function syncPayload(): LinkSync | null {
    if (!syncOn) return null;
    const s: LinkSync = { mode: "add_only" };
    if (importsOn) s.drive = { local: importsLocal, ...(importsFolder ? { folder_id: importsFolder.folder_id } : {}) };
    if (exportsOn && exportsFolder) s.exports = { local: exportsLocal, folder_id: exportsFolder.folder_id };
    return s;
  }

  /**
   * The parts of a sync block an edit changes, in one order: which lanes are on and their Frame.io
   * folders. The names on disk follow those folders, so they are sent again only when a lane or the
   * link's folder changes; an edit that touches neither never renames a folder on someone's disk.
   */
  const syncKey = (s: LinkSync | null | undefined) =>
    s ? JSON.stringify([s.drive ? s.drive.folder_id ?? "" : null, s.exports ? s.exports.folder_id ?? "" : null]) : "off";

  /** Whether the sync block needs sending: the switch, a lane, a lane's folder or the link's folder changed. */
  function syncChanged(): boolean {
    if (syncKey(syncPayload()) !== syncKey(up?.folder?.sync ?? null)) return true;
    return syncOn && !!folder && folder.folder_id !== up?.folder?.folder_id;
  }

  function body(): Record<string, unknown> {
    const b: Record<string, unknown> = {
      name: name.trim() || null,
      brand_display_name: title.trim() || null,
      brand_subtitle: subtitle.trim() || null,
      verify_email: otp,
      notify_emails: notifyEmails.trim() || null,
      remind_before_expiry: remind && expiry !== "never",
    };
    if (expiresAt !== "keep") b.expires_at = expiresAt ? expiresAt.toISOString() : null;
    if (pwMode === "set" && password) b.password = password;
    if (pwMode === "clear") b.password = "";
    if (kind === "upload") {
      // The FioTransfer sync rides inside `folder`, so the folder is sent whenever either changes.
      // A folder sent without a `sync` key keeps the stored one; clearing the switch sends
      // `sync: null`, which removes the block.
      const sync = syncPayload();
      const changed = syncChanged();
      const folderChanged = !editing || !!folder && (folder.folder_id !== up?.folder?.folder_id || folder.account_id !== up?.folder?.account_id);
      if (folder && (folderChanged || changed)) {
        const { sync: _stored, ...where } = folder;
        b.folder = { ...where, ...(changed ? { sync } : {}) };
      }
      // The project folder goes with the sync. Turning the sync off drops it on the server, so a
      // link that isn't synced doesn't turn into a two-way mirror.
      if (syncOn && (!editing || turbosort.trim() !== (up?.turbosort ?? ""))) b.turbosort = turbosort.trim() || null;
      b.send_link = sendLink;
      b.uploader_fields_required = { name: askName || tplAsks.name, email: askEmail || otp, message: askMessage, project: askProject || tplAsks.project };
      b.allowed_extensions = allowedExtensions;
      b.max_file_size = maxSizeGb ? Math.round(parseFloat(maxSizeGb) * 1e9) : null;
      b.subfolder_template = template.trim() || null;
    } else {
      if (!editing || JSON.stringify(source?.source) !== JSON.stringify(down?.source)) b.source = source?.source;
      b.viewer_fields_required = { name: askName, email: askEmail || otp };
      b.max_downloads = maxDownloads ? parseInt(maxDownloads, 10) : null;
      b.allow_preview = preview;
    }
    return b;
  }

  const ready = $derived(kind === "upload" ? !!folder : !!source);

  async function save() {
    error = null;
    if (!ready) {
      error = kind === "upload" ? "Choose the Frame.io folder uploads should land in." : "Choose the files or folder to send.";
      return;
    }
    if (expiry === "custom" && !customDate) {
      error = "Pick the date the link should stop working.";
      return;
    }
    if (pwMode === "set" && !password) {
      error = "Enter a password, or turn the password off.";
      return;
    }
    if (receivePlan || exportsPlan) {
      saving = true;
      try {
        await realizePlans();
      } catch (e) {
        error = e instanceof Error ? e.message : "Could not make the folders in Frame.io";
        return;
      } finally {
        saving = false;
      }
    }
    if (exportsMissing) {
      error = "Choose the Frame.io folder Exports go up to, or turn Exports off.";
      return;
    }
    if (laneIssue) {
      error = laneIssue;
      return;
    }
    saving = true;
    try {
      const base = kind === "upload" ? "/api/upload-links" : "/api/download-links";
      const res = await fetch(editing ? `${base}/${link!.id}` : base, {
        method: editing ? "PATCH" : "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body()),
      });
      if (!res.ok) {
        error = await errorDetail(res, editing ? "Could not save" : "Could not create the link");
        return;
      }
      const out = await res.json();
      if (editing) {
        toast("Saved");
        await invalidateAll();
        seed();
      } else {
        created = out;
        copied = await copyWithToast(out.public_url, "Link created and copied");
      }
    } finally {
      saving = false;
    }
  }

  async function sendEmail() {
    const l = created ?? link;
    if (!l || !emailTo.trim()) return;
    emailSending = true;
    try {
      const res = await fetch("/api/links/email", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ kind: l.kind, id: l.id, to: emailTo.trim(), note: emailNote.trim() || null }),
      });
      if (!res.ok) {
        toast(await errorDetail(res, "Could not send the email"), { kind: "error" });
        return;
      }
      toast(`Sent to ${emailTo.trim()}`);
      emailOpen = false;
      emailTo = "";
      emailNote = "";
    } finally {
      emailSending = false;
    }
  }

  function createAnother() {
    created = null;
    emailOpen = false;
    nameTouched = false;
    title = "";
    password = "";
    pwMode = "none";
    name = "";
    if (kind === "download") source = null;
  }

  async function revoke() {
    if (!link) return;
    const base = link.kind === "upload" ? "/api/upload-links" : "/api/download-links";
    const res = await fetch(`${base}/${link.id}/revoke`, { method: "POST" });
    if (!res.ok) return toast(await errorDetail(res, "Could not revoke"), { kind: "error" });
    await invalidateAll();
    seed();
    toast(`“${link.name}” revoked`, {
      action: {
        label: "Undo",
        run: async () => {
          await fetch(`${base}/${link!.id}/restore`, { method: "POST" });
          await invalidateAll();
          seed();
        },
      },
    });
  }

  async function restore() {
    if (!link) return;
    const base = link.kind === "upload" ? "/api/upload-links" : "/api/download-links";
    await fetch(`${base}/${link.id}/restore`, { method: "POST" });
    await invalidateAll();
    seed();
    toast("Link restored");
  }

  async function remove() {
    if (!link) return;
    const base = link.kind === "upload" ? "/api/upload-links" : "/api/download-links";
    const res = await fetch(`${base}/${link.id}`, { method: "DELETE" });
    if (!res.ok) return toast(await errorDetail(res, "Could not delete"), { kind: "error" });
    toast(`Deleted “${link.name}”`);
    await goto("/links");
  }

  const chip = (on: boolean) =>
    `inline-flex h-7 items-center gap-1.5 rounded-full border px-3 text-[12.5px] font-medium transition-colors ${on ? "border-accent/50 bg-accent-soft text-accent-hover" : "border-border text-muted hover:border-border-strong hover:text-text"}`;
  const input = "h-8 w-full rounded-md border border-border bg-bg px-2.5 text-[13px] text-text outline-none placeholder:text-faint hover:border-border-strong focus:border-accent focus:ring-3 focus:ring-accent-soft";
  const label = "pt-2 text-xs font-medium text-faint";
  const line = "grid grid-cols-1 gap-2 border-t border-border py-3.5 sm:grid-cols-[104px_minmax(0,1fr)] sm:gap-4";

  const previewFiles = $derived(
    ((source?.source as { names?: string[] } | undefined)?.names ?? []).slice(0, 3),
  );
  const shownLink = $derived(created ?? link);
</script>

<div class="grid gap-6 lg:grid-cols-[minmax(0,1fr)_340px]">
  <!-- ── form ── -->
  <div class="min-w-0 rounded-xl border border-border bg-surface text-[13px]">
    {#if created}
      <div class="grid justify-items-start gap-4 px-5 py-10 sm:px-7">
        <span class="grid h-10 w-10 place-items-center rounded-lg bg-success/15 text-success"><Icon name="check" size={20} /></span>
        <h2 class="text-lg font-semibold text-text">{copied ? "Link ready and copied" : "Link ready"}</h2>
        <div class="flex w-full max-w-xl items-center gap-2 rounded-lg border border-accent/40 bg-accent-soft py-1.5 pr-1.5 pl-3">
          <button type="button" class="min-w-0 flex-1 truncate text-left font-mono text-[13px] text-text hover:text-accent-hover" title="Copy link" onclick={() => copyWithToast(created!.public_url)}>{created.public_url.replace(/^https?:\/\//, "")}</button>
          <button type="button" class="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-surface-2 px-3 font-medium text-text hover:bg-surface-3" onclick={() => copyWithToast(created!.public_url)}>
            <Icon name="copy" size={15} />Copy
          </button>
        </div>
        {#if created.kind === "upload" && (created as UploadLink).return_link}
          {@const ret = (created as UploadLink).return_link!}
          <div class="grid w-full max-w-xl gap-1.5">
            <span class="text-xs text-faint">Send folder link</span>
            <div class="flex items-center gap-2 rounded-lg border border-border bg-surface-2/60 py-1.5 pr-1.5 pl-3">
              <button type="button" class="min-w-0 flex-1 truncate text-left font-mono text-[13px] text-text hover:text-accent-hover" title="Copy link" onclick={() => copyWithToast(ret.public_url)}>{ret.public_url.replace(/^https?:\/\//, "")}</button>
              <button type="button" class="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-surface-2 px-3 font-medium text-text hover:bg-surface-3" onclick={() => copyWithToast(ret.public_url)}>
                <Icon name="copy" size={15} />Copy
              </button>
            </div>
          </div>
        {/if}
        <div class="flex flex-wrap gap-2">
          <a href={created.public_url} target="_blank" rel="noopener" class="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-surface-2 px-3 font-medium text-text hover:bg-surface-3">
            <Icon name="open" size={15} />Open client page
          </a>
          <button type="button" class="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-surface-2 px-3 font-medium text-text hover:bg-surface-3 disabled:opacity-40" disabled={!emailConfigured} title={emailConfigured ? undefined : "Set up email in Settings first"} onclick={() => (emailOpen = !emailOpen)}>
            <Icon name="mail" size={15} />Email to client…
          </button>
          <button type="button" class="inline-flex h-8 items-center rounded-md px-3 font-medium text-muted hover:bg-surface-3 hover:text-text" onclick={createAnother}>Create another</button>
          <a href="/links" class="inline-flex h-8 items-center rounded-md px-3 font-medium text-muted hover:bg-surface-3 hover:text-text">Back to links</a>
        </div>
      </div>
    {:else}
      <div class="flex items-center gap-3 border-b border-border px-5 py-3.5 sm:px-7">
        <div class="min-w-0 flex-1">
          <h1 class="truncate text-base font-semibold text-text">{editing ? link?.name : "New link"}</h1>
          {#if editing && link}
            <div class="mt-0.5 flex min-w-0 items-center gap-2 text-xs text-faint">
              <button type="button" class="truncate font-mono hover:text-accent-hover" title="Copy link" onclick={() => copyWithToast(link!.public_url)}>{link.public_url.replace(/^https?:\/\//, "")}</button>
            </div>
          {/if}
        </div>
        {#if editing && link}
          <button type="button" class="grid h-8 w-8 place-items-center rounded-md text-muted hover:bg-surface-3 hover:text-text" title="Copy link" aria-label="Copy link" onclick={() => copyWithToast(link!.public_url)}><Icon name="copy" /></button>
          <a href={link.public_url} target="_blank" rel="noopener" class="grid h-8 w-8 place-items-center rounded-md text-muted hover:bg-surface-3 hover:text-text" title="Open client page" aria-label="Open client page"><Icon name="open" /></a>
        {/if}
        <a href="/links" class="grid h-8 w-8 place-items-center rounded-md text-faint hover:bg-surface-3 hover:text-text" aria-label="Close"><Icon name="x" /></a>
      </div>

      <div class="px-5 pt-4 pb-2 sm:px-7">
        {#if error}<p class="mb-3 rounded-md bg-danger/10 px-3 py-2 text-danger">{error}</p>{/if}
        {#if editing && down?.return_for}
          <p class="mb-3 flex items-center gap-3 rounded-md bg-surface-2 px-3 py-2 text-muted">
            <span class="flex-1">Managed by its receive link.</span>
            <a href="/upload-links/{down.return_for}" class="font-medium text-accent-hover hover:underline">Open the receive link</a>
          </p>
        {/if}
        {#if editing && link?.state === "revoked"}
          <p class="mb-3 flex items-center gap-3 rounded-md bg-surface-2 px-3 py-2 text-muted">
            <span class="flex-1">This link is revoked.</span>
            <button type="button" class="font-medium text-accent-hover hover:underline" onclick={restore}>Restore</button>
          </p>
        {/if}

        {#if !editing}
          <div class="mb-1 grid grid-cols-1 gap-2.5 sm:grid-cols-2" role="radiogroup" aria-label="Link type">
            {#each [{ k: "upload", icon: "receive", t: "Receive files" }, { k: "download", icon: "send", t: "Send files" }] as o (o.k)}
              <button
                type="button"
                role="radio"
                aria-checked={kind === o.k}
                onclick={() => setKind(o.k as Kind)}
                class="flex items-center gap-3 rounded-lg border px-3.5 py-3 text-left transition-colors {kind === o.k ? 'border-accent/50 bg-accent-soft' : 'border-border bg-surface-2/40 hover:border-border-strong'}"
              >
                <span class="grid h-9 w-9 shrink-0 place-items-center rounded-lg {kind === o.k ? 'bg-accent text-on-accent' : 'bg-surface-3 text-body'}"><Icon name={o.icon as "receive" | "send"} size={18} /></span>
                <b class="text-[13.5px] font-semibold text-text">{o.t}</b>
              </button>
            {/each}
          </div>
        {/if}

        {#if kind === "upload"}
          <div class="{line} {editing ? 'border-t-0' : ''}">
            <div class={label}>Into</div>
            <div class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-2">
              <FolderField bind:value={folder} {recents} title="Receive into" chooseVerb="Use" onchange={onFolder} />
              {#if recents.length}
                <div class="flex flex-wrap items-center gap-1.5 text-xs text-faint">
                  <span class="mr-0.5">Recent</span>
                  {#each recents.filter((r) => r.folder_id !== folder?.folder_id).slice(0, 4) as r (r.account_id + r.folder_id)}
                    <button type="button" class="{chip(false)} h-6 px-2.5 text-xs" onclick={() => { folder = r; onFolder(r); }}>{crumbNames(r).slice(-2).join(" / ")}</button>
                  {/each}
                </div>
              {/if}
            </div>
          </div>
        {:else}
          <div class="{line} {editing ? 'border-t-0' : ''}">
            <div class={label}>Files</div>
            <div class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-2">
              <button type="button" onclick={() => (sourceOpen = true)} class="flex h-8 w-full min-w-0 items-center gap-1.5 rounded-md border border-border bg-bg pr-2 pl-2 text-left hover:border-border-strong">
                <span class="text-faint"><Icon name={source?.source.type === "folder" ? "folder" : "file"} /></span>
                {#if source}
                  {@const where = crumbNames({ account_id: "", folder_id: "", path: source.path })}
                  <span class="min-w-0 flex-1 truncate text-muted">{where.join(" / ")}{#if source.source.type !== "folder"}{where.length ? " / " : ""}<span class="text-text">{source.label}</span>{/if}</span>
                {:else}
                  <span class="flex-1 truncate text-faint">Choose files or a folder…</span>
                {/if}
                <span class="text-faint"><Icon name="chevron" size={14} /></span>
              </button>
              {#if source}
                <div class="flex min-w-0 items-start gap-2 text-[12.5px] text-muted">
                  <span class="mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full bg-success"></span>
                  <span class="min-w-0 [overflow-wrap:anywhere]">
                    {sourceSummary(source.source as DownloadLink["source"])}
                  </span>
                </div>
              {/if}
            </div>
          </div>
        {/if}

        <div class={line}>
          <div class={label}><label for="lc-name">Name</label></div>
          <div class="grid grid-cols-[minmax(0,1fr)] gap-1.5">
            <input id="lc-name" class={input} bind:value={name} oninput={() => (nameTouched = true)} placeholder={kind === "upload" ? "e.g. Day 3 camera cards" : "e.g. Grade v4 selects"} autocomplete="off" />
          </div>
        </div>

        <div class={line}>
          <div class={label}><label for="lc-title">Page</label></div>
          <div class="grid gap-2 sm:grid-cols-2">
            <input id="lc-title" class={input} bind:value={title} placeholder={kind === "upload" ? "Send us your files" : "Your files are ready"} aria-label="Page title" autocomplete="off" />
            <input class={input} bind:value={subtitle} placeholder="Subtitle (optional)" aria-label="Page subtitle" autocomplete="off" />
          </div>
        </div>

        <div class={line}>
          <div class={label}>Expires</div>
          <div class="grid grid-cols-[minmax(0,1fr)] gap-2">
            <div class="flex flex-wrap gap-1.5" role="radiogroup" aria-label="Expiry">
              {#if editing && link?.expires_at}
                <button type="button" role="radio" aria-checked={expiry === "keep"} class={chip(expiry === "keep")} onclick={() => (expiry = "keep")}>{savedDay(link.expires_at)}</button>
              {/if}
              {#each [["1", "1 day"], ["7", "7 days"], ["30", "30 days"], ["never", "Never"], ["custom", "Pick a date…"]] as [v, l] (v)}
                <button type="button" role="radio" aria-checked={expiry === v} class={chip(expiry === v)} onclick={() => (expiry = v as Expiry)}>{l}</button>
              {/each}
            </div>
            {#if expiry === "custom"}
              <input type="date" class="{input} max-w-48" bind:value={customDate} aria-label="Last day" />
            {/if}
            {#if expiry !== "never"}
              <label class="flex items-center gap-2 text-muted">
                <input type="checkbox" bind:checked={remind} />
                Email me a day before it expires
              </label>
            {/if}
          </div>
        </div>

        <div class={line}>
          <div class={label}>Protect</div>
          <div class="grid gap-3">
            <div class="flex items-center gap-3">
              <div class="min-w-0 flex-1">
                <b class="block font-medium text-text">Email code</b>
              </div>
              <Switch bind:checked={otp} label="Email code" />
            </div>
            <div class="flex items-center gap-3">
              <div class="min-w-0 flex-1">
                <b class="block font-medium text-text">Password</b>
                {#if pwMode === "clear"}<small class="block text-xs text-faint">Removed when you save</small>{/if}
              </div>
              {#if pwMode === "keep"}
                <button type="button" class="text-xs font-medium text-muted hover:text-text" onclick={() => { pwMode = "set"; password = passphrase(); }}>Change</button>
                <button type="button" class="text-xs font-medium text-muted hover:text-danger" onclick={() => (pwMode = "clear")}>Remove</button>
              {:else if pwMode === "clear"}
                <button type="button" class="text-xs font-medium text-muted hover:text-text" onclick={() => (pwMode = "keep")}>Keep it</button>
              {:else}
                <Switch
                  checked={pwMode === "set"}
                  label="Password"
                  onchange={(v) => {
                    pwMode = v ? "set" : link?.password_protected ? "clear" : "none";
                    if (v && !password) password = passphrase();
                  }}
                />
              {/if}
            </div>
            {#if pwMode === "set"}
              <input class="{input} max-w-72 font-mono" bind:value={password} aria-label="Password" autocomplete="off" spellcheck="false" />
            {/if}
            <div class="flex flex-wrap items-center gap-1.5">
              <span class="mr-1 text-xs text-faint">Ask for</span>
              <button type="button" class="{chip(askName || tplAsks.name)} h-6 px-2.5 text-xs disabled:cursor-default" disabled={tplAsks.name} title={tplAsks.name ? "Used in Organize" : undefined} onclick={() => (askName = !askName)}>Name</button>
              <button type="button" class="{chip(askEmail || otp)} h-6 px-2.5 text-xs disabled:cursor-default" disabled={otp} title={otp ? "Needed for the email code" : undefined} onclick={() => (askEmail = !askEmail)}>Email</button>
              {#if kind === "upload"}
                <button type="button" class="{chip(askProject || tplAsks.project)} h-6 px-2.5 text-xs disabled:cursor-default" disabled={tplAsks.project} title={tplAsks.project ? "Used in Organize" : undefined} onclick={() => (askProject = !askProject)}>Project</button>
                <button type="button" class="{chip(askMessage)} h-6 px-2.5 text-xs" onclick={() => (askMessage = !askMessage)}>Message</button>
              {/if}
            </div>
          </div>
        </div>

        <div>
          {#if kind === "upload"}
            <div class={line}>
              <div class={label}>File types</div>
              <div class="grid grid-cols-[minmax(0,1fr)] gap-2">
                <div class="flex flex-wrap gap-1.5">
                  {#each Object.entries(TYPE_GROUPS) as [g, def] (g)}
                    <button type="button" aria-pressed={types.has(g as TypeGroup)} class={chip(types.has(g as TypeGroup))} onclick={() => toggleType(g as TypeGroup)}>{def.label}</button>
                  {/each}
                  <button type="button" aria-pressed={types.has("any")} class={chip(types.has("any"))} onclick={() => toggleType("any")}>Anything</button>
                </div>
                {#if allowedExtensions}<div class="text-xs text-faint">Accepts {allowedExtensions.map((e) => `.${e}`).join(" ")}</div>{/if}
              </div>
            </div>
            <div class={line}>
              <div class={label}><label for="lc-size">Max file size</label></div>
              <div class="relative max-w-44">
                <input id="lc-size" type="number" min="0" step="0.1" class="{input} pr-10" bind:value={maxSizeGb} placeholder="No limit" />
                <span class="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-xs text-faint">GB</span>
              </div>
            </div>
            <div class={line}>
              <div class={label}><label for="lc-tpl">Organize</label></div>
              <div class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-2">
                <input id="lc-tpl" class="{input} font-mono text-xs" bind:value={template} placeholder={"e.g. {date}/{uploader_name}"} autocomplete="off" spellcheck="false" />
                <div class="flex flex-wrap gap-1.5">
                  {#each UPLOAD_TOKENS as t (t.name)}
                    <button type="button" title={t.desc} class="{chip(template.includes(`{${t.name}}`))} h-6 px-2 font-mono text-[11.5px]" onclick={() => insertToken(t.name)}>{`{${t.name}}`}</button>
                  {/each}
                </div>
                <div class="rounded-md border border-border bg-surface-2/60 px-2.5 py-1.5 font-mono text-[11.5px] break-all text-muted">
                  {tplExample.join(" / ")} / <span class="text-text">A001_C003_1019XY.mov</span>
                </div>
              </div>
            </div>
            <div class={line}>
              <div class={label}>FioTransfer</div>
              <div class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-1.5">
                <div class="flex items-center gap-3">
                  <b class="min-w-0 flex-1 font-medium text-text">Sync this project's folders with FioTransfer</b>
                  <Switch bind:checked={syncOn} label="Sync this project's folders with FioTransfer" />
                </div>
                {#if !syncOn && !up?.folder?.sync && up?.turbosort}
                  <p class="text-xs text-faint">FioTransfer mirrors this link both ways as <span class="font-mono">{up.turbosort}</span>. Turn the sync on to split it into Imports and Exports.</p>
                {/if}
              </div>
            </div>
            {#if syncOn}
              {#if !folder}
                <div class={line}>
                  <div class={label}>Project</div>
                  <p class="pt-1.5 text-xs text-faint">Choose the folder uploads land in first.</p>
                </div>
              {:else}
                {#if syncError}
                  <p class="my-1 rounded-md bg-danger/10 px-3 py-2 text-danger">
                    {syncError}{#if /not connected/i.test(syncError)}. <a href="/settings" class="font-medium underline">Connect Frame.io in Settings</a>{/if}
                  </p>
                {/if}
                <div class={line}>
                  <div class={label}><label for="lc-project-folder">Local Project Folder</label></div>
                  <div class="min-w-0">
                    <input id="lc-project-folder" class={input} list="lc-project-folders" bind:value={turbosort} oninput={() => (turbosortTouched = true)} placeholder={job ? diskName(job.name) : "Project folder"} autocomplete="off" spellcheck="false" />
                    <datalist id="lc-project-folders">
                      {#each projectFolders as f (f)}<option value={f}></option>{/each}
                    </datalist>
                  </div>
                </div>
                <div class={line}>
                  <div class={label}>Imports</div>
                  <div class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-2">
                    <div class="flex min-w-0 items-center gap-3">
                      <b class="min-w-0 flex-1 font-medium text-text">Receive the client's files</b>
                      <Switch id="lc-sync-imports-on" bind:checked={importsOn} label="Receive the client's files" />
                    </div>
                    {#if importsOn}
                      <div class="flex min-w-0 items-center gap-1.5">
                        <div class="min-w-0 flex-1">
                          <FolderField id="lc-sync-imports" bind:value={importsFolder} startAt={job?.ref ?? folder} title="Imports come down from" chooseVerb="Use" placeholder={receivePlan && job ? `${job.name} / ${receivePlan.name}${receivePlan.existing ? "" : " (new)"}` : "This link's folder"} />
                        </div>
                        {#if importsFolder}
                          <button type="button" class="grid h-8 w-8 shrink-0 place-items-center rounded-md text-faint hover:bg-surface-3 hover:text-text" title="This link's folder" aria-label="Use this link's folder" onclick={() => (importsFolder = null)}><Icon name="x" /></button>
                        {/if}
                      </div>
                      {#if importsMissUploads && importsFolder}
                        {@const from = crumbNames(importsFolder).at(-1) ?? "that folder"}
                        <div class="flex flex-wrap items-center gap-2 rounded-md bg-warning/10 px-2.5 py-1.5 text-xs text-warning">
                          <span>The client's uploads land in <span class="font-mono">{crumbNames(folder).at(-1)}</span>, not in <span class="font-mono">{from}</span>, so they won't come down.</span>
                          <button type="button" class="h-7 rounded-md border border-warning/40 px-2.5 font-medium hover:bg-warning/10" onclick={receiveIntoImports}>Receive into {from} instead</button>
                        </div>
                      {/if}
                    {/if}
                  </div>
                </div>
                <div class={line}>
                  <div class={label}>Exports</div>
                  <div class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-2">
                    <div class="flex min-w-0 items-center gap-3">
                      <b class="min-w-0 flex-1 font-medium text-text">Send your exports</b>
                      <Switch id="lc-sync-exports-on" bind:checked={exportsOn} label="Send your exports" />
                    </div>
                    {#if exportsOn}
                      <div class="flex min-w-0 items-center gap-1.5">
                        <div class="min-w-0 flex-1">
                          <FolderField id="lc-sync-exports" bind:value={exportsFolder} startAt={job?.ref ?? folder} title="Exports go up to" chooseVerb="Use" placeholder={exportsPlan && job ? `${job.name} / ${exportsPlan.name}${exportsPlan.existing ? "" : " (new)"}` : "Choose the Frame.io folder"} />
                        </div>
                        {#if exportsFolder}
                          <button type="button" class="grid h-8 w-8 shrink-0 place-items-center rounded-md text-faint hover:bg-surface-3 hover:text-text" title="Choose another folder" aria-label="Clear the Exports folder" onclick={() => (exportsFolder = null)}><Icon name="x" /></button>
                        {/if}
                      </div>
                      {#if exportsMissing && job && !jobTakesExports()}
                        <div class="text-xs text-faint">Exports can't sit inside the folder Imports come down from: choose a folder outside it.</div>
                      {/if}
                      <div class="flex items-center gap-3 pt-1">
                        <b class="min-w-0 flex-1 font-medium text-text">Create Download Link</b>
                        <Switch id="lc-sync-send-link" bind:checked={sendLink} label="Create Download Link" />
                      </div>
                      {#if sendLink && exportsFolder && up?.return_link && exportsFolder.folder_id === up.folder?.sync?.exports?.folder_id}
                        <div class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted">
                          Clients download it at
                          <button type="button" class="min-w-0 truncate font-mono text-[11.5px] text-text hover:text-accent-hover" title="Copy link" onclick={() => copyWithToast(up!.return_link!.public_url)}>{up.return_link.public_url.replace(/^https?:\/\//, "")}</button>
                          <button type="button" class="font-medium text-accent-hover hover:underline" onclick={() => copyWithToast(up!.return_link!.public_url)}>Copy</button>
                        </div>
                      {/if}
                    {/if}
                    {#if laneIssue}
                      <div class="text-xs text-danger">{laneIssue}</div>
                    {/if}
                  </div>
                </div>
              {/if}
            {/if}
          {:else}
            <div class={line}>
              <div class={label}><label for="lc-maxdl">Limit</label></div>
              <div class="relative max-w-52">
                <input id="lc-maxdl" type="number" min="1" class="{input} pr-24" bind:value={maxDownloads} placeholder="Unlimited" />
                <span class="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-xs text-faint">downloads</span>
              </div>
            </div>
            <div class={line}>
              <div class={label}>Previews</div>
              <div class="flex items-center gap-3">
                <div class="min-w-0 flex-1">
                  <b class="block font-medium text-text">Show thumbnails</b>
                </div>
                <Switch bind:checked={preview} label="Show thumbnails" />
              </div>
            </div>
          {/if}
          <!-- A send link emails nobody but the expiry reminder, so it only asks who when that's on. -->
          {#if kind === "upload" || (remind && expiry !== "never")}
            <div class={line}>
              <div class={label}><label for="lc-notify">Notify</label></div>
              <div class="grid grid-cols-[minmax(0,1fr)] gap-1.5">
                <input id="lc-notify" class={input} bind:value={notifyEmails} placeholder={defaultNotify ? `${defaultNotify} (default)` : "you@example.com"} autocomplete="off" spellcheck="false" />
              </div>
            </div>
          {/if}
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-2 border-t border-border px-5 py-3.5 sm:px-7">
        {#if presetNaming}
          <input
            class="h-8 w-48 rounded-md border border-border bg-bg px-2.5 text-[13px] outline-none focus:border-accent"
            bind:value={presetName}
            placeholder="Preset name"
            aria-label="Preset name"
            onkeydown={(e) => {
              if (e.key === "Enter") void savePreset();
              if (e.key === "Escape") presetNaming = false;
            }}
          />
          <button type="button" class="h-8 rounded-md px-3 font-medium text-text hover:bg-surface-3 disabled:opacity-40" disabled={!presetName.trim()} onclick={savePreset}>Save</button>
          <button type="button" class="h-8 rounded-md px-2 text-muted hover:text-text" onclick={() => (presetNaming = false)}>Cancel</button>
        {:else}
          <label for="lc-preset" class="text-xs text-faint">Preset</label>
          <select id="lc-preset" class="h-8 max-w-56 rounded-md border border-border bg-bg px-2 text-[13px]" bind:value={preset} onchange={() => applyPreset(preset)}>
            <option value="">{kindPresets.length ? "Apply…" : "None yet"}</option>
            {#each kindPresets as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
            <option value="__save">Save these settings as a preset…</option>
          </select>
        {/if}
        <span class="flex-1"></span>
        {#if editing && link}
          {#if link.state !== "revoked"}
            <ConfirmButton label="Revoke" confirmLabel="Revoke link" onconfirm={revoke} />
          {/if}
          <ConfirmButton label="Delete" confirmLabel="Delete forever" onconfirm={remove} />
        {:else}
          <a href="/links" class="inline-flex h-8 items-center rounded-md px-3 font-medium text-muted hover:bg-surface-3 hover:text-text">Cancel</a>
        {/if}
        <button type="button" class="inline-flex h-8 items-center rounded-md bg-accent px-4 font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50" disabled={saving} onclick={save}>
          {saving ? (editing ? "Saving…" : "Creating…") : editing ? "Save changes" : "Create link"}
        </button>
      </div>
    {/if}

    {#if emailOpen && shownLink}
      <div class="grid gap-2.5 border-t border-border px-5 py-4 sm:px-7">
        <div class="text-xs font-medium text-faint">Email this link</div>
        <input class={input} type="email" bind:value={emailTo} placeholder="client@example.com" aria-label="Recipient" />
        <textarea class="{input} h-20 py-2" bind:value={emailNote} placeholder="A short note (optional)" aria-label="Note"></textarea>
        <div class="flex gap-2">
          <button type="button" class="inline-flex h-8 items-center rounded-md bg-accent px-4 font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50" disabled={!emailTo.trim() || emailSending} onclick={sendEmail}>{emailSending ? "Sending…" : "Send"}</button>
          <button type="button" class="inline-flex h-8 items-center rounded-md px-3 font-medium text-muted hover:bg-surface-3 hover:text-text" onclick={() => (emailOpen = false)}>Cancel</button>
        </div>
      </div>
    {/if}
  </div>

  <!-- ── preview ── -->
  <aside class="min-w-0 lg:sticky lg:top-24 lg:self-start" aria-label="Client page preview">
    <div class="mb-2 flex items-center justify-between">
      <span class="font-mono text-[10.5px] tracking-[0.08em] text-faint uppercase">Client sees</span>
      {#if editing && link && !created}
        <button type="button" class="text-xs text-muted hover:text-text disabled:opacity-40" disabled={!emailConfigured || link.state !== "ok"} title={emailConfigured ? undefined : "Set up email in Settings first"} onclick={() => (emailOpen = !emailOpen)}>Email to client…</button>
      {/if}
    </div>
    <div class="overflow-hidden rounded-xl border border-border bg-bg">
      <div class="flex h-8 items-center gap-2 border-b border-border bg-surface px-3">
        <span class="flex gap-1"><i class="h-2 w-2 rounded-full bg-surface-3"></i><i class="h-2 w-2 rounded-full bg-surface-3"></i><i class="h-2 w-2 rounded-full bg-surface-3"></i></span>
        {#if shownLink}
          <button type="button" class="truncate font-mono text-[10.5px] text-faint hover:text-accent-hover" title="Copy link" onclick={() => copyWithToast(shownLink!.public_url)}>{shownLink.public_url.replace(/^https?:\/\//, "")}</button>
        {:else}
          <span class="truncate font-mono text-[10.5px] text-faint">…/{kind === "upload" ? "u" : "d"}/new-link</span>
        {/if}
      </div>
      <div class="grid grid-cols-[minmax(0,1fr)] gap-2.5 px-6 pt-6 pb-5 text-center text-[12.5px] [overflow-wrap:anywhere]">
        {#if logoUrl}
          <img src={logoUrl} alt="" class="mx-auto mb-1 h-8 max-w-full object-contain" />
        {:else}
          <div class="mb-1 text-[13px] font-bold tracking-[0.16em] text-text uppercase">{brandName}</div>
        {/if}
        <div class="text-base font-semibold break-words text-text">{title.trim() || (kind === "upload" ? "Send us your files" : "Your files are ready")}</div>
        {#if subtitle.trim()}<div class="-mt-1.5 text-muted">{subtitle}</div>{/if}
        {#if pwMode === "set" || pwMode === "keep"}<div class="flex h-8 items-center rounded-md border border-border px-2.5 text-left text-faint">Password</div>{/if}
        {#if askName || tplAsks.name}<div class="flex h-8 items-center rounded-md border border-border px-2.5 text-left text-faint">Your name</div>{/if}
        {#if askEmail || otp}<div class="flex h-8 items-center rounded-md border border-border px-2.5 text-left text-faint">Email</div>{/if}
        {#if kind === "upload" && (askProject || tplAsks.project)}<div class="flex h-8 items-center rounded-md border border-border px-2.5 text-left text-faint">Project</div>{/if}
        {#if kind === "upload" && askMessage}<div class="flex h-8 items-center rounded-md border border-border px-2.5 text-left text-faint">Message</div>{/if}
        {#if otp}<div class="-mt-1 text-[11.5px] text-faint">We'll email you a 6-digit code</div>{/if}
        {#if kind === "upload"}
          <div class="grid justify-items-center gap-1 rounded-lg border-[1.5px] border-dashed border-border-strong px-3 py-5 text-muted">
            <span style="color:{accent}"><Icon name="up" size={20} /></span>
            <b class="text-[13px] font-medium text-text">Drop files or folders</b>
            <span>{typesLabel}{maxSizeGb ? ` · up to ${maxSizeGb} GB each` : ""}</span>
          </div>
          <div class="grid h-9 place-items-center rounded-md text-[13px] font-semibold" style="background:{accent};color:{accentText(accent)}">Upload</div>
        {:else}
          <div class="grid grid-cols-[minmax(0,1fr)] gap-1.5 text-left">
            {#each previewFiles.length ? previewFiles : ["Your files appear here"] as f, i (i)}
              <div class="flex min-w-0 items-center gap-2 rounded-md border border-border p-1.5">
                {#if preview && previewFiles.length}
                  <span class="h-6 w-10 shrink-0 rounded bg-gradient-to-br from-[#3a2a1c] via-[#1e2f3a] to-[#0f1720]"></span>
                {:else}
                  <span class="grid h-6 w-6 shrink-0 place-items-center rounded bg-surface-2 text-faint"><Icon name="file" size={13} /></span>
                {/if}
                <span class="min-w-0 flex-1 text-left [overflow-wrap:anywhere] {previewFiles.length ? 'text-text' : 'text-faint'}">{f}</span>
              </div>
            {/each}
          </div>
          <div class="grid h-9 place-items-center rounded-md text-[13px] font-semibold" style="background:{accent};color:{accentText(accent)}">
            Download all{source?.bytes ? ` · ${formatBytes(source.bytes)}` : ""}
          </div>
        {/if}
        {#if clientExpiry}<div class="-mt-1 text-[11px] text-faint">{clientExpiry}</div>{/if}
      </div>
    </div>
    {#if editing && link}
      <p class="mt-2 text-xs text-faint">{kindNoun(link.kind)} link · created {new Date(link.created_at).toLocaleDateString()}</p>
    {/if}
  </aside>
</div>

{#if kind === "download"}
  <FrameioPicker
    bind:open={sourceOpen}
    mode="source"
    title="Send files"
    start={source?.path?.length ? { account_id: source.source.account_id, folder_id: source.source.type === "folder" ? source.source.folder_id : source.path[source.path.length - 1].id, path: source.path } : (recents[0] ?? null)}
    {recents}
    onsource={onSource}
  />
{/if}
