<script lang="ts">
  import { goto, invalidateAll } from "$app/navigation";
  import { page } from "$app/state";
  import { ConfirmButton, Icon, Modal, SetRow, Switch } from "$lib/components";
  import { copyWithToast, errorDetail, toast } from "$lib/toast.svelte";
  import { registerPasskey } from "$lib/webauthn";
  import Presets from "./Presets.svelte";

  let { data } = $props();
  const email = $derived(data.email);
  const frameio = $derived(data.frameio);
  const frameioConfig = $derived(data.frameioConfig);
  // Adobe's "Redirect URI pattern" field is a regex; escape the redirect URI's metacharacters
  // (mainly the dots) so it matches that exact callback URL.
  const redirectUriPattern = $derived((frameioConfig.redirect_uri ?? "").replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  const security = $derived(data.security);
  const branding = $derived(data.branding);

  const flash = $derived(page.url.searchParams.get("frameio"));
  const input = "h-8 w-full rounded-md border border-border bg-bg px-2.5 text-[13px] text-text outline-none placeholder:text-faint hover:border-border-strong focus:border-accent focus:ring-3 focus:ring-accent-soft";
  const btn = "inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-surface-2 px-3 font-medium text-text hover:bg-surface-3 disabled:opacity-50";
  const primary = "inline-flex h-8 items-center rounded-md bg-accent px-4 font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50";

  const sections = [
    ["frameio", "Frame.io"],
    ["fiotransfer", "FioTransfer"],
    ["email", "Email"],
    ["branding", "Branding"],
    ["presets", "Link presets"],
    ["security", "Sign-in & security"],
    ["privacy", "Data & privacy"],
  ];

  // --- FioTransfer -----------------------------------------------------------
  // The names a synced link's Imports and Exports folders get unless the link names its own, so a
  // naming convention is set once. Links keep the names they were saved with.
  const fiotransfer = $derived(data.fiotransfer);
  let importsName = $state("");
  let exportsName = $state("");
  let savingLanes = $state(false);
  let lanesSeeded = false;
  $effect(() => {
    void fiotransfer;
    if (!lanesSeeded) {
      lanesSeeded = true;
      importsName = fiotransfer.imports_folder;
      exportsName = fiotransfer.exports_folder;
    }
  });
  async function saveLanes() {
    savingLanes = true;
    try {
      const res = await fetch("/api/settings/fiotransfer", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ imports_folder: importsName.trim() || null, exports_folder: exportsName.trim() || null }),
      });
      if (!res.ok) return toast(await errorDetail(res, "Could not save"), { kind: "error" });
      const out = await res.json();
      importsName = out.imports_folder;
      exportsName = out.exports_folder;
      toast("Folder names saved");
      await invalidateAll();
    } finally {
      savingLanes = false;
    }
  }

  // --- Frame.io --------------------------------------------------------------
  let disconnecting = $state(false);
  async function disconnect() {
    disconnecting = true;
    try {
      await fetch("/api/frameio/disconnect", { method: "POST" });
      await goto("/settings", { replaceState: true });
      await invalidateAll();
      toast("Frame.io disconnected");
    } finally {
      disconnecting = false;
    }
  }
  const fmt = (iso: string | null | undefined) => (iso ? new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) : "—");

  let frameioClientId = $state("");
  let frameioClientSecret = $state(""); // blank = keep stored
  let savingFrameioCfg = $state(false);
  let cfgSeeded = false;
  $effect(() => {
    void frameioConfig;
    if (!cfgSeeded) {
      cfgSeeded = true;
      frameioClientId = frameioConfig.client_id ?? "";
    }
  });
  async function saveFrameioConfig() {
    savingFrameioCfg = true;
    try {
      const res = await fetch("/api/settings/frameio-config", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ client_id: frameioClientId.trim(), client_secret: frameioClientSecret || null }),
      });
      if (!res.ok) return toast(await errorDetail(res, "Could not save"), { kind: "error" });
      frameioClientSecret = "";
      toast("Frame.io app credentials saved");
      await invalidateAll();
    } finally {
      savingFrameioCfg = false;
    }
  }

  // --- Email (SMTP) ----------------------------------------------------------
  let smtpHost = $state("");
  let smtpPort = $state(587);
  let smtpUsername = $state("");
  let smtpPassword = $state(""); // blank = keep stored
  let smtpFrom = $state("");
  let smtpUseTls = $state(false);
  let smtpStarttls = $state(true);
  let notifyEmail = $state("");
  let savingEmail = $state(false);
  let testing = $state(false);
  let emailSeeded = false;
  $effect(() => {
    void email;
    if (!emailSeeded) {
      emailSeeded = true;
      smtpHost = email.smtp_host ?? "";
      smtpPort = email.smtp_port ?? 587;
      smtpUsername = email.smtp_username ?? "";
      smtpFrom = email.smtp_from ?? "";
      smtpUseTls = email.smtp_use_tls;
      smtpStarttls = email.smtp_starttls;
      notifyEmail = email.notify_email ?? "";
    }
  });
  async function saveEmail() {
    savingEmail = true;
    try {
      const res = await fetch("/api/settings/email", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          smtp_host: smtpHost.trim(),
          smtp_port: smtpPort,
          smtp_username: smtpUsername.trim(),
          smtp_password: smtpPassword || null,
          smtp_from: smtpFrom.trim(),
          smtp_use_tls: smtpUseTls,
          smtp_starttls: smtpStarttls,
          notify_email: notifyEmail.trim(),
        }),
      });
      if (!res.ok) return toast(await errorDetail(res, "Could not save"), { kind: "error" });
      smtpPassword = "";
      toast("Email settings saved");
      await invalidateAll();
    } finally {
      savingEmail = false;
    }
  }
  async function sendTest() {
    testing = true;
    try {
      const res = await fetch("/api/settings/email/test", { method: "POST" });
      if (!res.ok) return toast(await errorDetail(res, "Could not send"), { kind: "error", ms: 12000 });
      toast(`Test email sent to ${email.notify_email}`);
    } finally {
      testing = false;
    }
  }

  // --- Default brand ---------------------------------------------------------
  let brandName = $state("");
  let brandAccent = $state("");
  let savingBrand = $state(false);
  let logoVersion = $state(0);
  let brandSeeded = false;
  $effect(() => {
    void branding;
    if (!brandSeeded) {
      brandSeeded = true;
      brandName = branding.brand_display_name ?? "";
      brandAccent = branding.brand_accent_color ?? "";
    }
  });
  const logoSrc = $derived(`/api/branding/logo?v=${branding.logo_updated_at ?? ""}${logoVersion}`);
  async function saveBrand() {
    savingBrand = true;
    try {
      const res = await fetch("/api/settings/branding", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ brand_display_name: brandName, brand_accent_color: brandAccent }),
      });
      if (!res.ok) return toast(await errorDetail(res, "Could not save"), { kind: "error" });
      toast("Brand saved");
      await invalidateAll();
    } finally {
      savingBrand = false;
    }
  }
  async function uploadLogo(e: Event) {
    const el = e.target as HTMLInputElement;
    const file = el.files?.[0];
    el.value = "";
    if (!file) return;
    const fd = new FormData();
    fd.append("file", file);
    const res = await fetch("/api/settings/branding/logo", { method: "POST", body: fd });
    if (!res.ok) return toast(await errorDetail(res, "Upload failed"), { kind: "error" });
    logoVersion = Date.now();
    toast("Logo updated");
    await invalidateAll();
  }
  async function removeLogo() {
    await fetch("/api/settings/branding/logo", { method: "DELETE" });
    logoVersion = Date.now();
    await invalidateAll();
  }

  // --- Password --------------------------------------------------------------
  let currentPassword = $state("");
  let newPassword = $state("");
  let confirmPassword = $state("");
  let savingPw = $state(false);
  async function changePassword() {
    if (newPassword.length < 8) return toast("Use at least 8 characters for the new password.", { kind: "error" });
    if (newPassword !== confirmPassword) return toast("The new passwords don't match.", { kind: "error" });
    savingPw = true;
    try {
      const res = await fetch("/api/settings/password", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
      });
      if (!res.ok) return toast(await errorDetail(res, "Could not change the password"), { kind: "error" });
      currentPassword = newPassword = confirmPassword = "";
      toast("Password changed");
    } finally {
      savingPw = false;
    }
  }

  // --- Passkeys --------------------------------------------------------------
  let pkNaming = $state(false);
  let pkName = $state("");
  let pkBusy = $state(false);
  async function addPasskey() {
    pkBusy = true;
    try {
      await registerPasskey(pkName.trim() || "Passkey");
      pkNaming = false;
      pkName = "";
      toast("Passkey added");
      await invalidateAll();
    } catch (e) {
      toast(e instanceof Error ? e.message : "Could not add the passkey", { kind: "error" });
    } finally {
      pkBusy = false;
    }
  }
  async function removePasskey(id: string) {
    await fetch(`/api/auth/passkeys/${id}`, { method: "DELETE" });
    toast("Passkey removed");
    await invalidateAll();
  }

  // --- Two-factor ------------------------------------------------------------
  let totpOpen = $state(false);
  let totpStep = $state<"qr" | "codes">("qr");
  let totpData = $state<{ secret: string; otpauth_uri: string; qr_svg: string } | null>(null);
  let totpCode = $state("");
  let totpError = $state<string | null>(null);
  let recoveryCodes = $state<string[]>([]);
  let totpBusy = $state(false);
  let disabling = $state(false);
  let disablePassword = $state("");
  async function startTotp() {
    totpError = null;
    totpCode = "";
    recoveryCodes = [];
    totpStep = "qr";
    totpBusy = true;
    try {
      const res = await fetch("/api/settings/totp/begin", { method: "POST" });
      if (!res.ok) return toast(await errorDetail(res, "Could not start the setup"), { kind: "error" });
      totpData = await res.json();
      totpOpen = true;
    } finally {
      totpBusy = false;
    }
  }
  async function confirmTotp() {
    totpError = null;
    totpBusy = true;
    try {
      const res = await fetch("/api/settings/totp/enable", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ code: totpCode }),
      });
      if (!res.ok) {
        totpError = await errorDetail(res, "That code didn't match");
        return;
      }
      recoveryCodes = (await res.json()).recovery_codes;
      totpStep = "codes";
      await invalidateAll();
    } finally {
      totpBusy = false;
    }
  }
  async function disableTotp() {
    const res = await fetch("/api/settings/totp/disable", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ password: disablePassword }),
    });
    if (!res.ok) return toast(await errorDetail(res, "Could not turn it off"), { kind: "error" });
    disabling = false;
    disablePassword = "";
    toast("Two-factor is off");
    await invalidateAll();
  }

  // --- Data & privacy --------------------------------------------------------
  let subjectEmail = $state("");
  let eraseMode = $state<"anonymize" | "delete">("anonymize");
  let dsarBusy = $state(false);
  async function exportSubject() {
    const e = subjectEmail.trim();
    if (!e) return;
    dsarBusy = true;
    try {
      const res = await fetch(`/api/privacy/subject?email=${encodeURIComponent(e)}`);
      if (!res.ok) return toast(await errorDetail(res, "Could not export"), { kind: "error" });
      const d = await res.json();
      const blob = new Blob([JSON.stringify(d, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `portal-data-${e}.json`;
      a.click();
      URL.revokeObjectURL(url);
      toast(`Exported ${d.upload_sessions.length} upload(s) and ${d.download_sessions.length} download(s)`);
    } finally {
      dsarBusy = false;
    }
  }
  async function eraseSubject() {
    const e = subjectEmail.trim();
    if (!e) return;
    dsarBusy = true;
    try {
      const res = await fetch("/api/privacy/subject/erase", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email: e, mode: eraseMode }),
      });
      if (!res.ok) return toast(await errorDetail(res, "Could not erase"), { kind: "error" });
      const r = await res.json();
      const word = eraseMode === "delete" ? "Deleted" : "Anonymised";
      toast(`${word} ${r.upload_sessions} upload(s), ${r.download_sessions} download(s), ${r.email_verifications} verification(s)`);
    } finally {
      dsarBusy = false;
    }
  }

  async function logout() {
    await fetch("/api/auth/logout", { method: "POST" });
    await goto("/login");
  }
</script>

{#snippet card(id: string, title: string, desc: string | null, body: import("svelte").Snippet)}
  <section {id} class="scroll-mt-24">
    <h2 class="text-sm font-semibold text-text">{title}</h2>
    {#if desc}<p class="mt-0.5 max-w-2xl text-faint">{desc}</p>{/if}
    <div class="mt-3 overflow-hidden rounded-xl border border-border bg-surface">{@render body()}</div>
  </section>
{/snippet}

<div class="mx-auto grid max-w-5xl gap-8 text-[13px] lg:grid-cols-[170px_minmax(0,1fr)]">
  <nav class="hidden self-start lg:sticky lg:top-24 lg:grid lg:gap-0.5" aria-label="Settings sections">
    {#each sections as [id, label] (id)}
      <a href="#{id}" class="rounded-md px-2.5 py-1.5 font-medium text-muted hover:bg-surface-2 hover:text-text">{label}</a>
    {/each}
    <a href="/activity" class="mt-3 rounded-md px-2.5 py-1.5 text-faint hover:bg-surface-2 hover:text-text">Admin log →</a>
  </nav>

  <div class="min-w-0 space-y-9">
    <h1 class="text-xl font-semibold tracking-tight">Settings</h1>

    {#if flash === "connected"}
      <p class="rounded-md bg-success/10 px-3 py-2 text-success">Frame.io connected.</p>
    {:else if flash === "error"}
      <p class="rounded-md bg-danger/10 px-3 py-2 text-danger">Frame.io connection failed. Please try again.</p>
    {:else if flash === "unconfigured"}
      <p class="rounded-md bg-warning/10 px-3 py-2 text-warning">Add your Frame.io app credentials below before connecting.</p>
    {/if}

    <!-- Frame.io -->
    {#snippet frameioBody()}
      <SetRow label="Connection" help={frameio.connected ? undefined : "Links and the picker need it."}>
        <div class="flex flex-wrap items-center gap-3">
          <span class="flex min-w-0 flex-1 items-center gap-2">
            <span class="h-2 w-2 shrink-0 rounded-full {frameio.connected ? 'bg-success' : 'bg-warning'}"></span>
            {#if frameio.connected}
              <span class="min-w-0"><span class="text-text">Connected as {frameio.adobe_email ?? "your account"}</span><span class="block text-xs text-faint">Token {frameio.needs_refresh ? "renews on next use" : `valid until ${fmt(frameio.expires_at)}`}</span></span>
            {:else}
              <span class="text-muted">Not connected</span>
            {/if}
          </span>
          {#if frameio.connected}
            <ConfirmButton label={disconnecting ? "Disconnecting…" : "Disconnect"} confirmLabel="Disconnect Frame.io" onconfirm={disconnect} disabled={disconnecting} class="border border-border text-text" />
          {:else if frameioConfig.configured}
            <a href="/api/frameio/oauth/connect" data-sveltekit-reload class={primary}>Connect Frame.io</a>
          {/if}
        </div>
      </SetRow>
      <SetRow label="Client ID" for="s-cid" help="From your Adobe Developer Console “OAuth Web App” credential.">
        <input id="s-cid" class={input} bind:value={frameioClientId} placeholder="e.g. a1b2c3…" spellcheck="false" />
      </SetRow>
      <SetRow label="Client secret" for="s-secret" help={frameioConfig.has_secret ? "Leave empty to keep the saved one." : undefined}>
        <input id="s-secret" type="password" class={input} bind:value={frameioClientSecret} autocomplete="off" data-bwignore data-1p-ignore data-lpignore="true" placeholder={frameioConfig.has_secret ? "••••••••" : "Client secret"} />
      </SetRow>
      <SetRow label="Redirect URI" help="Adobe's “Default redirect URI”.">
        <div class="flex gap-2"><input class="{input} font-mono text-xs opacity-80" value={frameioConfig.redirect_uri} readonly aria-label="Redirect URI" /><button type="button" class={btn} onclick={() => copyWithToast(frameioConfig.redirect_uri, "Copied")}><Icon name="copy" size={14} />Copy</button></div>
      </SetRow>
      <SetRow label="Redirect URI pattern" help="Adobe's required “Redirect URI pattern”.">
        <div class="flex gap-2"><input class="{input} font-mono text-xs opacity-80" value={redirectUriPattern} readonly aria-label="Redirect URI pattern" /><button type="button" class={btn} onclick={() => copyWithToast(redirectUriPattern, "Copied")}><Icon name="copy" size={14} />Copy</button></div>
      </SetRow>
      <div class="flex items-center gap-3 border-t border-border px-5 py-3">
        <span class="flex-1"></span>
        <button type="button" class={primary} disabled={savingFrameioCfg} onclick={saveFrameioConfig}>{savingFrameioCfg ? "Saving…" : "Save credentials"}</button>
      </div>
      <details class="border-t border-border px-5 py-3" open={!frameio.connected}>
        <summary class="cursor-pointer font-medium text-muted hover:text-text">How to set up the Adobe credential</summary>
        <ol class="mt-2 ml-5 list-decimal space-y-1.5 text-muted">
          <li>Open the <a href="https://developer.adobe.com/console" target="_blank" rel="noopener" class="text-accent-hover hover:underline">Adobe Developer Console</a> and create a project.</li>
          <li>Add API → <b class="font-medium text-text">Frame.io</b>, and choose an <b class="font-medium text-text">OAuth Web App</b> credential (not Single-Page App or Server-to-Server).</li>
          <li>Paste the Redirect URI above as the default redirect URI, and the pattern into “Redirect URI pattern”.</li>
          <li>Copy the Client ID and Client secret here, save, then Connect Frame.io.</li>
          <li>Your Frame.io user must be linked to an Adobe ID; Frame.io's help explains how.</li>
        </ol>
      </details>
    {/snippet}
    {@render card("frameio", "Frame.io", null, frameioBody)}

    <!-- FioTransfer -->
    {#snippet fiotransferBody()}
      <SetRow label="Imports folder" for="s-imports" help="Where the client's files come down to, inside each project folder.">
        <input id="s-imports" class="{input} max-w-60 font-mono text-xs" bind:value={importsName} placeholder={fiotransfer.defaults.imports_folder} autocomplete="off" spellcheck="false" />
      </SetRow>
      <SetRow label="Exports folder" for="s-exports" help="What goes up to Frame.io from each project folder. A link finds its Exports by this name in Frame.io.">
        <input id="s-exports" class="{input} max-w-60 font-mono text-xs" bind:value={exportsName} placeholder={fiotransfer.defaults.exports_folder} autocomplete="off" spellcheck="false" />
      </SetRow>
      <div class="flex justify-end border-t border-border px-5 py-3">
        <button type="button" class={primary} disabled={savingLanes} onclick={saveLanes}>{savingLanes ? "Saving…" : "Save names"}</button>
      </div>
    {/snippet}
    {@render card("fiotransfer", "FioTransfer", "The folder names new synced links start with. Each link can still name its own; links you've saved keep theirs.", fiotransferBody)}

    <!-- Email -->
    {#snippet emailBody()}
      <SetRow label="Status">
        <span class="flex items-center gap-2"><span class="h-2 w-2 rounded-full {email.configured ? 'bg-success' : 'bg-faint'}"></span><span class={email.configured ? "text-text" : "text-muted"}>{email.configured ? "Sending email" : "Not set up"}</span></span>
      </SetRow>
      <SetRow label="SMTP server" for="s-host">
        <div class="flex gap-2"><input id="s-host" class={input} bind:value={smtpHost} placeholder="smtp.example.com" /><input type="number" class="{input} w-24" bind:value={smtpPort} min="1" max="65535" aria-label="Port" /></div>
      </SetRow>
      <SetRow label="Security">
        <div class="flex flex-wrap gap-x-5 gap-y-2">
          <label class="flex items-center gap-2 text-muted"><input type="checkbox" bind:checked={smtpStarttls} /> STARTTLS (port 587)</label>
          <label class="flex items-center gap-2 text-muted"><input type="checkbox" bind:checked={smtpUseTls} /> Implicit TLS (port 465)</label>
        </div>
      </SetRow>
      <SetRow label="From address" for="s-from"><input id="s-from" class={input} bind:value={smtpFrom} placeholder="portal@example.com" /></SetRow>
      <SetRow label="Username" for="s-user"><input id="s-user" class={input} bind:value={smtpUsername} autocomplete="off" placeholder="(optional)" /></SetRow>
      <SetRow label="Password" for="s-pass" help="iCloud, Gmail and other providers with two-factor sign-in need an app-specific password, not your account password.">
        <input id="s-pass" type="password" class={input} bind:value={smtpPassword} autocomplete="off" data-bwignore data-1p-ignore data-lpignore="true" placeholder={email.has_password ? "•••••••• (leave empty to keep it)" : "(optional)"} />
      </SetRow>
      <SetRow label="Notifications to" for="s-notify" help="Gets “upload finished” emails and reminders, unless a link names its own.">
        <input id="s-notify" class={input} bind:value={notifyEmail} placeholder="ops@example.com (defaults to admin)" />
      </SetRow>
      <div class="flex flex-wrap items-center justify-end gap-2 border-t border-border px-5 py-3">
        <button type="button" class={btn} disabled={testing || !email.configured} onclick={sendTest}>{testing ? "Sending…" : "Send a test email"}</button>
        <button type="button" class={primary} disabled={savingEmail} onclick={saveEmail}>{savingEmail ? "Saving…" : "Save email settings"}</button>
      </div>
    {/snippet}
    {@render card("email", "Email", null, emailBody)}

    <!-- Branding -->
    {#snippet brandBody()}
      <SetRow label="Name" for="s-bname" help="On link pages and emails when a link has no title of its own.">
        <input id="s-bname" class={input} bind:value={brandName} placeholder="e.g. FARBHAUS" />
      </SetRow>
      <SetRow label="Accent colour" for="s-baccent">
        <span class="flex items-center gap-2">
          <input type="color" class="h-8 w-10 cursor-pointer rounded border border-border bg-bg" value={brandAccent || "#f59e0b"} oninput={(e) => (brandAccent = (e.target as HTMLInputElement).value)} aria-label="Pick accent colour" />
          <input id="s-baccent" class="{input} max-w-40 font-mono" bind:value={brandAccent} placeholder="#f59e0b" spellcheck="false" />
        </span>
      </SetRow>
      <SetRow label="Logo" help="A PNG for link pages, emails and the sign-in screen.">
        <div class="flex flex-wrap items-center gap-3">
          <span class="grid h-12 w-28 place-items-center rounded-md border border-border bg-bg">
            {#if branding.has_logo}<img src={logoSrc} alt="Current logo" class="max-h-9 max-w-24 object-contain" />{:else}<span class="text-xs text-faint">No logo</span>{/if}
          </span>
          <label class="{btn} cursor-pointer">{branding.has_logo ? "Replace" : "Upload PNG"}<input type="file" accept="image/png,.png" class="hidden" onchange={uploadLogo} /></label>
          {#if branding.has_logo}<ConfirmButton label="Remove" confirmLabel="Remove logo" onconfirm={removeLogo} />{/if}
        </div>
      </SetRow>
      <div class="flex justify-end border-t border-border px-5 py-3">
        <button type="button" class={primary} disabled={savingBrand} onclick={saveBrand}>{savingBrand ? "Saving…" : "Save brand"}</button>
      </div>
    {/snippet}
    {@render card("branding", "Branding", null, brandBody)}

    {#snippet presetsBody()}<Presets presets={data.presets} />{/snippet}
    {@render card("presets", "Link presets", null, presetsBody)}

    <!-- Security -->
    {#snippet securityBody()}
      <SetRow label="Signed in as"><span class="font-mono text-xs text-text">{data.user.email}</span></SetRow>
      <SetRow label="Password" help="At least 8 characters.">
        <div class="grid gap-2 sm:grid-cols-3">
          <input type="password" class={input} bind:value={currentPassword} placeholder="Current" autocomplete="current-password" aria-label="Current password" />
          <input type="password" class={input} bind:value={newPassword} placeholder="New" autocomplete="new-password" aria-label="New password" />
          <input type="password" class={input} bind:value={confirmPassword} placeholder="New, again" autocomplete="new-password" aria-label="Confirm new password" />
        </div>
        <button type="button" class="{btn} mt-2" disabled={savingPw || !currentPassword || !newPassword} onclick={changePassword}>{savingPw ? "Saving…" : "Change password"}</button>
      </SetRow>
      <SetRow label="Passkeys" help="Sign in with Touch ID, Windows Hello or a security key.">
        <div class="grid gap-2">
          {#each security.passkeys as pk (pk.id)}
            <div class="flex items-center gap-3 rounded-md border border-border px-3 py-2">
              <span class="text-faint"><Icon name="lock" /></span>
              <span class="min-w-0 flex-1"><span class="block truncate text-text">{pk.name ?? "Passkey"}</span><span class="block text-xs text-faint">Added {new Date(pk.created_at).toLocaleDateString()}{pk.last_used_at ? ` · last used ${new Date(pk.last_used_at).toLocaleDateString()}` : ""}</span></span>
              <ConfirmButton label="Remove" confirmLabel="Remove passkey" onconfirm={() => removePasskey(pk.id)} />
            </div>
          {/each}
          {#if pkNaming}
            <div class="flex gap-2">
              <input class={input} bind:value={pkName} placeholder="e.g. MacBook Touch ID" aria-label="Passkey name" onkeydown={(e) => e.key === "Enter" && addPasskey()} />
              <button type="button" class={primary} disabled={pkBusy} onclick={addPasskey}>{pkBusy ? "Waiting…" : "Add"}</button>
              <button type="button" class="h-8 px-2 text-muted hover:text-text" onclick={() => (pkNaming = false)}>Cancel</button>
            </div>
          {:else}
            <button type="button" class="{btn} justify-self-start" onclick={() => (pkNaming = true)}><Icon name="plus" size={14} />Add a passkey</button>
          {/if}
        </div>
      </SetRow>
      <SetRow label="Two-factor" help="A 6-digit code from an authenticator app after your password.">
        <div class="flex flex-wrap items-center gap-3">
          <span class="flex items-center gap-2"><span class="h-2 w-2 rounded-full {security.totp_enabled ? 'bg-success' : 'bg-faint'}"></span>{security.totp_enabled ? "On" : "Off"}</span>
          {#if security.totp_enabled}
            {#if disabling}
              <input type="password" class="{input} max-w-56" bind:value={disablePassword} placeholder="Your password" aria-label="Password to turn off two-factor" onkeydown={(e) => e.key === "Enter" && disableTotp()} />
              <button type="button" class="{btn} text-danger" disabled={!disablePassword} onclick={disableTotp}>Turn off</button>
              <button type="button" class="h-8 px-2 text-muted hover:text-text" onclick={() => (disabling = false)}>Cancel</button>
            {:else}
              <button type="button" class={btn} onclick={() => (disabling = true)}>Turn off…</button>
            {/if}
          {:else}
            <button type="button" class={primary} disabled={totpBusy} onclick={startTotp}>{totpBusy ? "…" : "Turn on"}</button>
          {/if}
        </div>
      </SetRow>
      <SetRow label="Session">
        <button type="button" class={btn} onclick={logout}>Sign out</button>
      </SetRow>
    {/snippet}
    {@render card("security", "Sign-in & security", null, securityBody)}

    <!-- Data & privacy -->
    {#snippet privacyBody()}
      <SetRow label="Person" for="s-subject" help="Matching ignores case.">
        <input id="s-subject" type="email" class={input} bind:value={subjectEmail} placeholder="person@example.com" autocomplete="off" />
      </SetRow>
      <SetRow label="Export" help="A JSON copy of everything stored about them.">
        <button type="button" class={btn} disabled={dsarBusy || !subjectEmail.trim()} onclick={exportSubject}>{dsarBusy ? "Working…" : "Export data"}</button>
      </SetRow>
      <SetRow label="Erase" help="Anonymise keeps the transfer rows (so counts stay right) without names, emails, messages or IPs. Delete removes them. Pending codes for the address go either way.">
        <div class="flex flex-wrap items-center gap-2">
          <select class="{input} w-auto" bind:value={eraseMode} aria-label="How to erase">
            <option value="anonymize">Anonymise</option>
            <option value="delete">Delete</option>
          </select>
          <ConfirmButton label="Erase data" confirmLabel={eraseMode === "delete" ? "Delete everything for this email" : "Anonymise everything for this email"} onconfirm={eraseSubject} disabled={dsarBusy || !subjectEmail.trim()} class="border border-border text-danger" />
        </div>
      </SetRow>
    {/snippet}
    {@render card("privacy", "Data & privacy requests", null, privacyBody)}
  </div>
</div>

<Modal bind:open={totpOpen} title="Turn on two-factor">
  {#if totpStep === "qr" && totpData}
    <div class="space-y-3 text-sm">
      <p class="text-muted">Scan this with your authenticator app, then enter the 6-digit code to confirm.</p>
      <div class="mx-auto flex w-40 justify-center rounded-md bg-white p-3">
        <!-- Rendered as an <img> data URI rather than {@html} so the SVG can't act as an HTML sink. -->
        <img src={"data:image/svg+xml;utf8," + encodeURIComponent(totpData.qr_svg)} alt="TOTP QR code" class="w-full" />
      </div>
      <p class="break-all text-xs text-faint">Or enter this key: <span class="font-mono text-text">{totpData.secret}</span></p>
      <input bind:value={totpCode} inputmode="numeric" autocomplete="one-time-code" placeholder="123456" class="w-full rounded-md border border-border bg-surface-2 px-3 py-2 text-center text-lg tracking-widest" />
      {#if totpError}<p class="text-sm text-danger">{totpError}</p>{/if}
    </div>
  {:else if totpStep === "codes"}
    <div class="space-y-3 text-sm">
      <p class="text-muted">Save these recovery codes. Each works once.</p>
      <ul class="grid grid-cols-2 gap-1 rounded-md border border-border bg-surface-2 p-3 font-mono text-xs">
        {#each recoveryCodes as c (c)}<li>{c}</li>{/each}
      </ul>
      <button type="button" class={btn} onclick={() => copyWithToast(recoveryCodes.join("\n"), "Recovery codes copied")}><Icon name="copy" size={14} />Copy codes</button>
    </div>
  {/if}
  {#snippet footer()}
    {#if totpStep === "qr"}
      <button type="button" class="h-8 px-3 text-muted hover:text-text" disabled={totpBusy} onclick={() => (totpOpen = false)}>Cancel</button>
      <button type="button" class={primary} disabled={totpBusy || totpCode.length < 6} onclick={confirmTotp}>{totpBusy ? "Checking…" : "Confirm"}</button>
    {:else}
      <button type="button" class={primary} onclick={() => (totpOpen = false)}>Done</button>
    {/if}
  {/snippet}
</Modal>
