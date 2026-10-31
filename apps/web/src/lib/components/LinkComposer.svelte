<script lang="ts" module>
  export type Branding = { name: string | null; accent: string | null; logo_url: string | null };
  export type BrandProfile = {
    id: string;
    name: string;
    display_name: string | null;
    accent_color: string | null;
    has_logo: boolean;
    logo_url: string | null;
    links?: number;
  };
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
  import { coveringRules, createFolder, crumbNames, listFolders, nasPathFor, resolvePath, type DownloadSource, type FolderRef, type Named, type RuleLike } from "$lib/frameio";
  import { expiresText, kindNoun, sourceSummary, type DownloadLink, type LiveWorkSync, type UploadLink } from "$lib/links";
  import { copyWithToast, errorDetail, toast } from "$lib/toast.svelte";
  import { formatBytes, accentText } from "$lib/format";
  import { UPLOAD_TOKENS } from "$lib/tokens";
  import { rememberFolder } from "$lib/recents";

  type Kind = "upload" | "download";
  type Expiry = "keep" | "1" | "7" | "30" | "never" | "custom";

  let {
    mode = "create",
    kind: initialKind = "upload",
    link = null,
    recents = [],
    rules = [],
    projectFolders = [],
    branding,
    initialFolder = null,
    openSourcePicker = false,
    emailConfigured = true,
    presets = [],
    brands = [],
    defaultNotify = null,
  }: {
    mode?: "create" | "edit";
    kind?: Kind;
    link?: UploadLink | DownloadLink | null;
    recents?: FolderRef[];
    rules?: RuleLike[];
    /** Live_Work folder names already in use, for the project-folder drop-down. */
    projectFolders?: string[];
    branding: Branding;
    initialFolder?: FolderRef | null;
    openSourcePicker?: boolean;
    emailConfigured?: boolean;
    presets?: Preset[];
    brands?: BrandProfile[];
    defaultNotify?: string | null;
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
  let types = $state<Set<TypeGroup | "any">>(new Set(["any"]));
  let customExt = $state<string[]>([]);
  let maxSizeGb = $state("");
  let template = $state("");
  let turbosort = $state("");
  let syncOn = $state(false);
  let driveLocal = $state("1_DRIVE");
  let exportsLocal = $state("0_EXPORTS");
  let driveFolder = $state<FolderRef | null>(null);
  // The inbound lane. A new link brings the client's drive down (the owner's ask), and an existing
  // link follows what it stores: `drive` present means the switch is on.
  let importsOn = $state(true);
  // The outbound lane. A new link has one (the owner's ask: exports unless he says otherwise), and
  // it points at the link's own folder until a child of the project is chosen. An existing link
  // follows what it stores: `exports` present means the switch is on.
  let exportsOn = $state(true);
  let exportsFolder = $state<FolderRef | null>(null);
  // The two sync sides choose from the first level of the project only: the link's folder names
  // the account, and the project's root folder is the parent they list (and create into).
  let syncRoot = $state<{ account_id: string; folder_id: string; name: string } | null>(null);
  let children = $state<Named[] | null>(null);
  let loadingFolders = $state(false);
  let syncError = $state<string | null>(null);
  let newName = $state<{ drive: string; exports: string }>({ drive: "", exports: "" });
  let creating = $state<"drive" | "exports" | null>(null);
  let createIssue = $state<{ side: "drive" | "exports"; message: string } | null>(null);
  // Once a row has been chosen by hand (or asked for a new folder), the default-named folder
  // must not take the row back when the level is read again.
  let touched = $state<{ drive: boolean; exports: boolean }>({ drive: false, exports: false });
  let maxDownloads = $state("");
  let preview = $state(true);
  let preset = $state("");
  let brandProfileId = $state("");
  let notifyEmails = $state("");
  let remind = $state(false);
  let savedPresets = $state<Preset[]>(untrack(() => [...presets]));
  let presetNaming = $state(false);
  let presetName = $state("");
  let moreOpen = $state(false);
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
      brandProfileId = link.brand_profile_id ?? "";
      notifyEmails = (link.notify_emails ?? []).join(", ");
      remind = link.remind_before_expiry;
      if (link.kind === "upload") {
        folder = link.folder;
        askName = link.uploader_fields_required.name;
        askEmail = link.uploader_fields_required.email && !link.verify_email;
        askMessage = link.uploader_fields_required.message;
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
        turbosort = link.turbosort ?? "";
        const s = link.folder?.sync ?? null;
        syncOn = !!s;
        driveLocal = s?.drive?.local ?? "1_DRIVE";
        exportsLocal = s?.exports?.local ?? "0_EXPORTS";
        // Only the ids are stored; the breadcrumb is rebuilt once a folder is picked again.
        const account = link.folder?.account_id ?? "";
        driveFolder = s?.drive?.folder_id ? { account_id: account, folder_id: s.drive.folder_id } : null;
        exportsFolder = s?.exports?.folder_id ? { account_id: account, folder_id: s.exports.folder_id } : null;
        // An existing link is shown as it was saved: no `exports` keeps the switch off, so editing
        // it never adds an upload lane the link deliberately did not have.
        exportsOn = !!s?.exports;
        // Same for the inbound lane: no `drive` keeps the switch off, so editing it never starts
        // downloading a drive the link deliberately did not have.
        importsOn = !!s?.drive;
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
      name = initialFolder ? (crumbNames(initialFolder).at(-1) ?? "") : "";
    }
  }
  seed();

  onMount(() => {
    if (openSourcePicker && kind === "download" && !source) sourceOpen = true;
  });

  // ── derived ─────────────────────────────────────────────────────────────
  const coverage = $derived(kind === "upload" ? coveringRules(folder, rules) : []);
  const enabledCoverage = $derived(coverage.filter((c) => c.enabled));
  // The page brand: the chosen profile, with the app-wide brand filling what it leaves empty
  // (a profile's logo stands alone, as on the client page).
  const profile = $derived(brands.find((b) => b.id === brandProfileId) ?? null);
  const accent = $derived(profile?.accent_color || branding.accent || "#f59e0b");
  const brandName = $derived(profile?.display_name || branding.name || "Portal");
  const logoUrl = $derived(profile ? profile.logo_url : branding.logo_url);

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
  const fmtDay = (d: Date) => d.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" });
  const expiryHelp = $derived.by(() => {
    if (expiresAt === "keep") return link?.expires_at ? (link.state === "expired" ? `Expired ${fmtDay(new Date(link.expires_at))}. Pick a new length to reopen it.` : `Closes ${fmtDay(new Date(link.expires_at))}.`) : "";
    if (expiresAt === null) return expiry === "custom" ? "Pick the last day it should work." : "Stays open until you revoke it.";
    return `Closes ${fmtDay(expiresAt)} at 23:59.`;
  });
  const expiryShort = $derived.by(() => {
    if (expiresAt === "keep") return link ? `Open ${expiresText(link)}` : "";
    if (expiresAt === null) return expiry === "custom" ? "" : "No expiry";
    return `Open until ${fmtDay(expiresAt)}`;
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
    };
    const parts = template.trim() ? template.replace(/\{(\w+)\}/g, (m, k) => vals[k] ?? m).split("/").map((x) => x.trim()).filter(Boolean) : [];
    return [crumbNames(folder).at(-1) ?? "Folder", ...parts];
  });

  const moreSummary = $derived(
    kind === "upload"
      ? [typesLabel, maxSizeGb ? `up to ${maxSizeGb} GB per file` : "no size limit", template.trim() ? "sorted into folders" : ""].filter(Boolean).join(" · ")
      : [maxDownloads ? `${maxDownloads} downloads max` : "unlimited downloads", preview ? "thumbnails on" : "thumbnails off"].join(" · "),
  );
  const moreLine = $derived(notifyEmails.trim() ? `${moreSummary} · custom notifications` : moreSummary);

  // ── interactions ────────────────────────────────────────────────────────
  function setKind(k: Kind) {
    if (editing || k === kind) return;
    kind = k;
    if (!nameTouched) name = k === "upload" ? (crumbNames(folder).at(-1) ?? "") : (source?.label ?? "");
    if (k === "download" && !source) sourceOpen = true;
  }

  function onFolder(f: FolderRef) {
    if (!nameTouched) name = crumbNames(f).at(-1) ?? "";
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

  // ── the project's first level (sync) ────────────────────────────────────
  // A sync side is one folder of the project, so both rows list the folders directly inside the
  // project root. A name that is not there yet can be typed and Portal creates it.
  const FOLDER_NAME_RULE = "A folder name can't be empty, contain / \\ % or be . or ..";

  const folderKey = $derived(kind === "upload" && syncOn && folder ? `${folder.account_id}:${folder.folder_id}` : "");
  let folderSeq = 0;

  $effect(() => {
    const key = folderKey;
    untrack(() => void loadProjectFolders(key));
  });

  const byName = (a: Named, b: Named) => a.name.localeCompare(b.name);

  const sideRef = (side: "drive" | "exports") => (side === "drive" ? driveFolder : exportsFolder);

  /** What a side offers: the project's children, plus a saved folder that is no longer in the list. */
  function sideOptions(side: "drive" | "exports"): Named[] {
    const list = children ?? [];
    const chosen = sideRef(side);
    if (chosen && !list.some((c) => c.id === chosen.folder_id)) {
      return [...list, { id: chosen.folder_id, name: chosen.folder_name || "the saved folder" }];
    }
    return list;
  }

  /** Choose a folder id for one side ("" = the link's own folder, on both sides). */
  function selectSide(side: "drive" | "exports", id: string) {
    const f = folder;
    if (!f) return;
    if (!id) {
      if (side === "drive") driveFolder = null;
      else exportsFolder = null;
      return;
    }
    const name = sideOptions(side).find((c) => c.id === id)?.name ?? "";
    const ref: FolderRef = { account_id: f.account_id, folder_id: id, folder_name: name || null };
    if (side === "drive") {
      driveFolder = ref;
      if (name) driveLocal = name;
    } else {
      exportsFolder = ref;
      if (name) exportsLocal = name;
    }
  }

  /** A row the user changed: from now on its choice is theirs, not the 1_DRIVE/0_EXPORTS default. */
  function pickSide(side: "drive" | "exports", id: string) {
    touched[side] = true;
    selectSide(side, id);
  }

  /**
   * Both lanes on, and neither side names a folder: both would target this link's own folder.
   * The API takes it, but FioTransfer will not mix imports and exports in one Frame.io folder, so
   * the exports lane is skipped: say so here, before the link is saved, not later in the app.
   */
  const lanesCollide = $derived(syncOn && importsOn && exportsOn && !driveFolder && !exportsFolder && !loadingFolders);

  /** The sync validator's rule for a local folder name, applied before we ask Frame.io for one. */
  function cleanFolderName(raw: string): string | null {
    const v = raw.trim();
    if (!v || v === "." || v === ".." || /[/\\%]/.test(v) || /[\u0000-\u001f]/.test(v)) return null;
    return v;
  }

  async function loadProjectFolders(key: string) {
    const f = folder;
    if (!key || !f) {
      syncRoot = null;
      children = null;
      syncError = null;
      loadingFolders = false;
      return;
    }
    const my = ++folderSeq;
    loadingFolders = true;
    syncError = null;
    try {
      // The first level hangs off the project root. The picked folder usually carries its
      // breadcrumb; without one, ask Frame.io where the folder lives.
      let root = f.path?.length ? f.path[0].id : "";
      if (!root) root = (await resolvePath(f.account_id, f.folder_id)).root_folder_id ?? "";
      if (my !== folderSeq) return;
      if (!root) root = f.folder_id;
      const moved = !!syncRoot && syncRoot.folder_id !== root;
      if (moved || syncRoot?.folder_id !== root) {
        syncRoot = { account_id: f.account_id, folder_id: root, name: crumbNames(f)[0] ?? f.project_name ?? "this project" };
      }
      if (moved) {
        // Another project: the two chosen folders belong to the one we just left.
        driveFolder = null;
        exportsFolder = null;
        newName = { drive: "", exports: "" };
        createIssue = null;
        touched = { drive: false, exports: false };
      }
      const kids = await listFolders(f.account_id, root);
      if (my !== folderSeq) return;
      children = [...kids].sort(byName);
      preselectByName();
    } catch (e) {
      if (my !== folderSeq) return;
      children = [];
      syncError = e instanceof Error ? e.message : "Could not read the project's folders from Frame.io";
    } finally {
      if (my === folderSeq) loadingFolders = false;
    }
  }

  /**
   * The defaults stay `1_DRIVE` and `0_EXPORTS`: on a link with no id for a side, a child already
   * named so is preselected. A row the user has touched is left alone. A side whose switch is off,
   * or that has no such child, stays on the link's own folder.
   *
   * This runs when the level is read, and again when a side is switched on: a lane added to an
   * existing link points at its folder the moment it appears, not only on an untouched new link.
   */
  function preselectByName() {
    if (!children?.length) return;
    if (importsOn && !driveFolder && !touched.drive) {
      const c = children.find((x) => x.name === (driveLocal.trim() || "1_DRIVE"));
      if (c) selectSide("drive", c.id);
    }
    if (exportsOn && !exportsFolder && !touched.exports) {
      const c = children.find((x) => x.name === (exportsLocal.trim() || "0_EXPORTS"));
      if (c) selectSide("exports", c.id);
    }
  }

  /** Make the typed folder under the project root, then use it. A sibling of that name wins. */
  async function createSideFolder(side: "drive" | "exports") {
    const name = cleanFolderName(newName[side]);
    const root = syncRoot;
    if (!name) {
      createIssue = { side, message: FOLDER_NAME_RULE };
      return;
    }
    if (!root || creating) return;
    createIssue = null;
    touched[side] = true;
    const twin = children?.find((c) => c.name === name);
    if (twin) {
      // Same name, same parent: that is the folder meant, so select it instead of a duplicate.
      selectSide(side, twin.id);
      newName[side] = "";
      return;
    }
    creating = side;
    try {
      const made = await createFolder(root.account_id, root.folder_id, name);
      children = [...(children ?? []), made].sort(byName);
      selectSide(side, made.id);
      newName[side] = "";
      // Read the level back, so the list is Frame.io's answer and not our guess.
      void refreshFolders();
    } catch (e) {
      createIssue = { side, message: e instanceof Error ? e.message : "Could not create the folder" };
    } finally {
      creating = null;
    }
  }

  async function refreshFolders() {
    const root = syncRoot;
    if (!root) return;
    try {
      children = [...(await listFolders(root.account_id, root.folder_id))].sort(byName);
    } catch {
      // Keep what we have: the folder exists, this list is only the mirror.
    }
  }

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
      ask: { name: askName, email: askEmail, message: askMessage },
      subtitle: subtitle.trim(),
      brand_profile_id: brandProfileId || null,
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
    if (typeof st.subtitle === "string" && st.subtitle) subtitle = st.subtitle;
    brandProfileId = typeof st.brand_profile_id === "string" && brands.some((b) => b.id === st.brand_profile_id) ? st.brand_profile_id : "";
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
  /** `folder.sync` as the API stores it, or null when the switch is off. */
  function syncPayload(): LiveWorkSync | null {
    if (!syncOn) return null;
    return {
      mode: "add_only",
      // An unticked box is no lane at all on that side: the key is absent, which the API stores as
      // none. No folder picked means the link's own folder, on either side.
      ...(importsOn
        ? { drive: { local: driveLocal.trim() || "1_DRIVE", ...(driveFolder ? { folder_id: driveFolder.folder_id } : {}) } }
        : {}),
      ...(exportsOn
        ? { exports: { local: exportsLocal.trim() || "0_EXPORTS", ...(exportsFolder ? { folder_id: exportsFolder.folder_id } : {}) } }
        : {}),
    };
  }

  function body(): Record<string, unknown> {
    const b: Record<string, unknown> = {
      name: name.trim() || null,
      brand_display_name: title.trim() || null,
      brand_subtitle: subtitle.trim() || null,
      verify_email: otp,
      brand_profile_id: brandProfileId || null,
      notify_emails: notifyEmails.trim() || null,
      remind_before_expiry: remind && expiry !== "never",
    };
    if (expiresAt !== "keep") b.expires_at = expiresAt ? expiresAt.toISOString() : null;
    if (pwMode === "set" && password) b.password = password;
    if (pwMode === "clear") b.password = "";
    if (kind === "upload") {
      // The live-work sync rides inside `folder`, so the folder is sent whenever either changes.
      const sync = syncPayload();
      const syncChanged = JSON.stringify(sync) !== JSON.stringify(up?.folder?.sync ?? null);
      const folderChanged = !editing || !!folder && (folder.folder_id !== up?.folder?.folder_id || folder.account_id !== up?.folder?.account_id);
      if (folder && (folderChanged || syncChanged)) {
        // Clearing the switch sends `sync: null`, which removes the block.
        b.folder = { ...folder, ...(sync ? { sync } : up?.folder?.sync ? { sync: null } : {}) };
      }
      b.uploader_fields_required = { name: askName, email: askEmail || otp, message: askMessage };
      b.allowed_extensions = allowedExtensions;
      b.max_file_size = maxSizeGb ? Math.round(parseFloat(maxSizeGb) * 1e9) : null;
      b.subfolder_template = template.trim() || null;
      if (!editing || turbosort.trim() !== (up?.turbosort ?? "")) b.turbosort = turbosort.trim() || null;
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
    if (kind === "upload" && syncOn && importsOn && exportsOn && (exportsLocal.trim() || "0_EXPORTS") === (driveLocal.trim() || "1_DRIVE")) {
      error = "The client drive and exports need different local folders.";
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
    name = kind === "upload" ? (crumbNames(folder).at(-1) ?? "") : "";
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
        <p class="max-w-xl text-muted">
          {#if created.kind === "upload"}
            Receives into {crumbNames((created as UploadLink).folder).join(" / ")}.
          {:else}
            Sends {source?.label ?? "the files you picked"}.
          {/if}
          {expiryShort}{otp ? ", email code on" : ""}{pwMode === "set" ? ", password protected" : ""}.
        </p>
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
        {#if editing && link?.state === "revoked"}
          <p class="mb-3 flex items-center gap-3 rounded-md bg-surface-2 px-3 py-2 text-muted">
            <span class="flex-1">This link is revoked. Clients see a closed page.</span>
            <button type="button" class="font-medium text-accent-hover hover:underline" onclick={restore}>Restore</button>
          </p>
        {/if}

        {#if !editing}
          <div class="mb-1 grid grid-cols-1 gap-2.5 sm:grid-cols-2" role="radiogroup" aria-label="Link type">
            {#each [{ k: "upload", icon: "receive", t: "Receive files", s: "Clients upload into Frame.io" }, { k: "download", icon: "send", t: "Send files", s: "Clients download from Frame.io" }] as o (o.k)}
              <button
                type="button"
                role="radio"
                aria-checked={kind === o.k}
                onclick={() => setKind(o.k as Kind)}
                class="flex items-center gap-3 rounded-lg border px-3.5 py-3 text-left transition-colors {kind === o.k ? 'border-accent/50 bg-accent-soft' : 'border-border bg-surface-2/40 hover:border-border-strong'}"
              >
                <span class="grid h-9 w-9 shrink-0 place-items-center rounded-lg {kind === o.k ? 'bg-accent text-on-accent' : 'bg-surface-3 text-body'}"><Icon name={o.icon as "receive" | "send"} size={18} /></span>
                <span><b class="block text-[13.5px] font-semibold text-text">{o.t}</b><small class="block text-xs text-faint">{o.s}</small></span>
              </button>
            {/each}
          </div>
        {/if}

        {#if kind === "upload"}
          <div class="{line} {editing ? 'border-t-0' : ''}">
            <div class={label}>Into</div>
            <div class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-2">
              <FolderField bind:value={folder} {recents} title="Receive into" chooseVerb="Use" onchange={onFolder} />
              {#if folder}
                <div class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[12.5px] text-muted">
                  {#if enabledCoverage.length}
                    <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-success"></span>
                    Lands on the NAS at <span class="font-mono text-[11.5px] break-all text-text">{nasPathFor(folder, enabledCoverage[0], rules)}</span>
                    <span class="text-faint">via “{enabledCoverage[0].name}”</span>
                  {:else}
                    <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-faint"></span>
                    Stays in Frame.io only.
                    <a href="/sync-rules/new?account_id={encodeURIComponent(folder.account_id)}&folder_id={encodeURIComponent(folder.folder_id)}" class="text-accent-hover hover:underline">Sync this folder</a>
                  {/if}
                </div>
              {/if}
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
                    {#if source.source.type === "folder"}<span class="text-faint">· clients get what's in it when they open the link</span>{/if}
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
            <div class="text-xs text-faint">Only you see this. It's how the link shows up in your list.</div>
          </div>
        </div>

        <div class={line}>
          <div class={label}><label for="lc-title">Page</label></div>
          <div class="grid gap-2 sm:grid-cols-2">
            <input id="lc-title" class={input} bind:value={title} placeholder={kind === "upload" ? "Send us your files" : "Your files are ready"} aria-label="Page title" autocomplete="off" />
            <input class={input} bind:value={subtitle} placeholder="Subtitle (optional)" aria-label="Page subtitle" autocomplete="off" />
          </div>
        </div>

        {#if brands.length}
          <div class={line}>
            <div class={label}><label for="lc-brand">Brand</label></div>
            <div class="grid grid-cols-[minmax(0,1fr)] gap-1.5">
              <select id="lc-brand" class="{input} max-w-72" bind:value={brandProfileId}>
                <option value="">{branding.name ? `${branding.name} (default)` : "Default brand"}</option>
                {#each brands as b (b.id)}<option value={b.id}>{b.name}</option>{/each}
              </select>
              <div class="text-xs text-faint">The logo, colour and name on the client's page and emails.</div>
            </div>
          </div>
        {/if}

        <div class={line}>
          <div class={label}>Expires</div>
          <div class="grid grid-cols-[minmax(0,1fr)] gap-2">
            <div class="flex flex-wrap gap-1.5" role="radiogroup" aria-label="Expiry">
              {#if editing && link?.expires_at}
                <button type="button" role="radio" aria-checked={expiry === "keep"} class={chip(expiry === "keep")} onclick={() => (expiry = "keep")}>Keep {expiresText(link)}</button>
              {/if}
              {#each [["1", "1 day"], ["7", "7 days"], ["30", "30 days"], ["never", "Never"], ["custom", "Pick a date…"]] as [v, l] (v)}
                <button type="button" role="radio" aria-checked={expiry === v} class={chip(expiry === v)} onclick={() => (expiry = v as Expiry)}>{l}</button>
              {/each}
            </div>
            {#if expiry === "custom"}
              <input type="date" class="{input} max-w-48" bind:value={customDate} aria-label="Last day" />
            {/if}
            <div class="text-xs text-faint">{expiryHelp}</div>
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
                <small class="block text-xs text-faint">{kind === "upload" ? "Uploaders" : "Recipients"} confirm their email with a 6-digit code first</small>
              </div>
              <Switch bind:checked={otp} label="Email code" />
            </div>
            <div class="flex items-center gap-3">
              <div class="min-w-0 flex-1">
                <b class="block font-medium text-text">Password</b>
                <small class="block text-xs text-faint">
                  {pwMode === "keep" ? "A password is set." : pwMode === "clear" ? "The password will be removed when you save." : "Share it separately from the link."}
                </small>
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
              <button type="button" class="{chip(askName)} h-6 px-2.5 text-xs" onclick={() => (askName = !askName)}>Name</button>
              <button type="button" class="{chip(askEmail || otp)} h-6 px-2.5 text-xs disabled:cursor-default" disabled={otp} title={otp ? "Needed for the email code" : undefined} onclick={() => (askEmail = !askEmail)}>Email</button>
              {#if kind === "upload"}
                <button type="button" class="{chip(askMessage)} h-6 px-2.5 text-xs" onclick={() => (askMessage = !askMessage)}>Message</button>
              {/if}
            </div>
          </div>
        </div>

        <details class="border-t border-border" bind:open={moreOpen}>
          <summary class="flex cursor-pointer list-none items-center gap-1.5 py-3 font-medium text-muted hover:text-text [&::-webkit-details-marker]:hidden">
            <span class="inline-block w-3 transition-transform {moreOpen ? 'rotate-90' : ''}">›</span>
            More options <span class="truncate font-normal text-faint">· {moreLine}</span>
          </summary>
          {#if kind === "upload"}
            <div class="{line} border-t-0 pt-1">
              <div class={label}>File types</div>
              <div class="grid grid-cols-[minmax(0,1fr)] gap-2">
                <div class="flex flex-wrap gap-1.5">
                  {#each Object.entries(TYPE_GROUPS) as [g, def] (g)}
                    <button type="button" aria-pressed={types.has(g as TypeGroup)} class={chip(types.has(g as TypeGroup))} onclick={() => toggleType(g as TypeGroup)}>{def.label}</button>
                  {/each}
                  <button type="button" aria-pressed={types.has("any")} class={chip(types.has("any"))} onclick={() => toggleType("any")}>Anything</button>
                </div>
                <div class="text-xs text-faint">
                  {#if allowedExtensions}Accepts {allowedExtensions.map((e) => `.${e}`).join(" ")}{:else}Every file type is accepted.{/if}
                </div>
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
              <div class={label}>Project Folder</div>
              <div class="flex items-center gap-3">
                <div class="min-w-0 flex-1">
                  <b class="block font-medium text-text">Sync this project's folders with FioTransfer</b>
                  <small class="block text-xs text-faint">The client's drive comes down, your exports go up</small>
                </div>
                <Switch bind:checked={syncOn} label="Sync this project's folders with FioTransfer" />
              </div>
            </div>
            <div class={line}>
              <div class={label}><label for="lc-project-folder">Local folder</label></div>
              <div class="grid gap-1.5">
                <input id="lc-project-folder" class="{input} disabled:opacity-40" list="lc-project-folders" bind:value={turbosort} placeholder="Your project folder (leave empty for none)" autocomplete="off" disabled={!syncOn} />
                <datalist id="lc-project-folders">
                  {#each projectFolders as f (f)}
                    <option value={f}></option>
                  {/each}
                </datalist>
<p class="text-xs text-faint">The folder this project lives in on your machine: pick one already in use, or type a new name. FioTransfer reads it from Portal and keeps <span class="font-mono">1_DRIVE</span> and <span class="font-mono">0_EXPORTS</span> in step.</p>
              </div>
            </div>
            {#if syncOn}
              {#if !folder}
                <div class={line}>
                  <div class={label}>Folders</div>
                  <p class="text-xs text-faint">Choose the folder uploads land in first: both sync rows list that project's own folders.</p>
                </div>
              {:else}
                {#if syncError}
                  <p class="my-1 rounded-md bg-danger/10 px-3 py-2 text-danger">
                    {syncError}{#if /not connected/i.test(syncError)}. <a href="/settings" class="font-medium underline">Connect Frame.io in Settings</a>{/if}
                  </p>
                {/if}
                <div class={line}>
                  <div class={label}><label for="lc-sync-drive">Imports</label></div>
                  <div class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-2">
                    <div class="flex min-w-0 items-center gap-3">
                      <div class="min-w-0 flex-1">
                        <b class="block font-medium text-text">Download imports</b>
                        <small class="block text-xs text-faint">The client's drive comes down to your machine</small>
                      </div>
                      <Switch id="lc-sync-imports-on" bind:checked={importsOn} label="Download imports" onchange={(on) => on && preselectByName()} />
                    </div>
                    {#if importsOn}
                      <div class="grid gap-2 sm:grid-cols-2">
                        <select id="lc-sync-drive" class="{input} max-w-none" value={driveFolder?.folder_id ?? ""} disabled={loadingFolders} onchange={(e) => pickSide("drive", e.currentTarget.value)}>
                          {#if loadingFolders && !children}
                            <option value="">Loading the project's folders…</option>
                          {:else}
                            <option value="">This link's folder</option>
                            {#each sideOptions("drive") as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
                          {/if}
                        </select>
                        <input class={input} bind:value={driveLocal} placeholder="1_DRIVE" aria-label="Local folder for the client drive" autocomplete="off" spellcheck="false" />
                      </div>
                      <div class="flex min-w-0 flex-wrap items-center gap-1.5">
                        <input
                          class="{input} max-w-56"
                          bind:value={newName.drive}
                          placeholder="or type a new folder name"
                          aria-label="New folder name for the client drive"
                          autocomplete="off"
                          spellcheck="false"
                          onkeydown={(e) => {
                            if (e.key === "Enter") {
                              e.preventDefault();
                              void createSideFolder("drive");
                            }
                          }}
                        />
                        <button type="button" class="h-8 rounded-md border border-border px-3 font-medium text-text hover:bg-surface-3 disabled:cursor-default disabled:opacity-40" disabled={!cleanFolderName(newName.drive) || !!creating} onclick={() => createSideFolder("drive")}>
                          {creating === "drive" ? "Creating…" : "Create"}
                        </button>
                      </div>
                      {#if newName.drive.trim() && !cleanFolderName(newName.drive)}
                        <div class="text-xs text-danger">{FOLDER_NAME_RULE}</div>
                      {/if}
                      {#if createIssue?.side === "drive"}
                        <div class="text-xs text-danger">{createIssue.message}</div>
                      {/if}
                      <div class="text-xs text-faint">
                        {#if driveFolder}
                          Downloaded from <span class="font-mono">{syncRoot?.name ?? "the project"} / {driveFolder.folder_name || "that folder"}</span> into <span class="font-mono">{driveLocal.trim() || "1_DRIVE"}</span>
                        {:else}
                          Downloaded from this link's folder into <span class="font-mono">{driveLocal.trim() || "1_DRIVE"}</span>
                        {/if}
                      </div>
                    {:else}
                      <p class="text-xs text-faint">Nothing is downloaded. Turn on <span class="font-medium text-muted">Imports</span> to bring the client's drive down into <span class="font-mono">{driveLocal.trim() || "1_DRIVE"}</span>.</p>
                    {/if}
                  </div>
                </div>
                <div class={line}>
                  <div class={label}>Exports</div>
                  <div class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-2">
                    <div class="flex min-w-0 items-center gap-3">
                      <div class="min-w-0 flex-1">
                        <b class="block font-medium text-text">Upload exports</b>
                        <small class="block text-xs text-faint">Your exports go up to Frame.io</small>
                      </div>
                      <Switch id="lc-sync-exports-on" bind:checked={exportsOn} label="Upload exports" onchange={(on) => on && preselectByName()} />
                    </div>
                    {#if exportsOn}
                      <div class="grid gap-2 sm:grid-cols-2">
                        <select id="lc-sync-exports" class="{input} max-w-none" value={exportsFolder?.folder_id ?? ""} disabled={loadingFolders} onchange={(e) => pickSide("exports", e.currentTarget.value)}>
                          {#if loadingFolders && !children}
                            <option value="">Loading the project's folders…</option>
                          {:else}
                            <option value="">This link's folder</option>
                            {#each sideOptions("exports") as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
                          {/if}
                        </select>
                        <input class={input} bind:value={exportsLocal} placeholder="0_EXPORTS" aria-label="Local folder for exports" autocomplete="off" spellcheck="false" />
                      </div>
                      <div class="flex min-w-0 flex-wrap items-center gap-1.5">
                        <input
                          class="{input} max-w-56"
                          bind:value={newName.exports}
                          placeholder="or type a new folder name"
                          aria-label="New folder name for exports"
                          autocomplete="off"
                          spellcheck="false"
                          onkeydown={(e) => {
                            if (e.key === "Enter") {
                              e.preventDefault();
                              void createSideFolder("exports");
                            }
                          }}
                        />
                        <button type="button" class="h-8 rounded-md border border-border px-3 font-medium text-text hover:bg-surface-3 disabled:cursor-default disabled:opacity-40" disabled={!cleanFolderName(newName.exports) || !!creating} onclick={() => createSideFolder("exports")}>
                          {creating === "exports" ? "Creating…" : "Create"}
                        </button>
                      </div>
                      {#if newName.exports.trim() && !cleanFolderName(newName.exports)}
                        <div class="text-xs text-danger">{FOLDER_NAME_RULE}</div>
                      {/if}
                      {#if createIssue?.side === "exports"}
                        <div class="text-xs text-danger">{createIssue.message}</div>
                      {/if}
                      {#if lanesCollide}
                        <div class="text-xs text-danger">Pick the exports folder: the link's own folder is already where imports come from.</div>
                      {/if}
                      <div class="text-xs text-faint">
                        {#if exportsFolder}
                          Uploaded from <span class="font-mono">{exportsLocal.trim() || "0_EXPORTS"}</span> into <span class="font-mono">{syncRoot?.name ?? "the project"} / {exportsFolder.folder_name || "that folder"}</span>
                        {:else}
                          Uploaded from <span class="font-mono">{exportsLocal.trim() || "0_EXPORTS"}</span> into this link's folder
                        {/if}
                      </div>
                    {:else}
                      <p class="text-xs text-faint">Nothing is uploaded. Turn on <span class="font-medium text-muted">Upload exports</span> to send <span class="font-mono">{exportsLocal.trim() || "0_EXPORTS"}</span> up to Frame.io</p>
                    {/if}
                  </div>
                </div>
              {/if}
              <div class="{line} border-t-0 pt-1">
                <div></div>
                <p class="text-xs text-faint">Files are added in both directions. Nothing is ever deleted: you can clear Frame.io once you have your own copy. Both rows list the folders directly inside {syncRoot ? `“${syncRoot.name}”` : "the project"}: pick one, or type a new name and Portal makes it.</p>
              </div>
            {/if}
          {:else}
            <div class="{line} border-t-0 pt-1">
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
                  <small class="block text-xs text-faint">Turn off for unreleased material</small>
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
                <div class="text-xs text-faint">
                  {kind === "upload" ? "Who gets the email when an upload finishes" : "Who gets the expiry reminder"}{kind === "upload" && remind && expiry !== "never" ? " and the expiry reminder" : ""}. Separate several addresses with commas.
                </div>
              </div>
            </div>
          {/if}
        </details>
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
        {#if askName}<div class="flex h-8 items-center rounded-md border border-border px-2.5 text-left text-faint">Your name</div>{/if}
        {#if askEmail || otp}<div class="flex h-8 items-center rounded-md border border-border px-2.5 text-left text-faint">Email</div>{/if}
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
        {#if expiryShort}<div class="text-[11px] text-faint">{expiryShort}</div>{/if}
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
