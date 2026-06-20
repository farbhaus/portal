<script lang="ts">
  import { goto, invalidateAll } from "$app/navigation";
  import { page } from "$app/state";
  import { Card, Button, StatusPill, PageHeader, Modal } from "$lib/components";
  import { registerPasskey } from "$lib/webauthn";

  let { data } = $props();
  const email = $derived(data.email);
  const frameio = $derived(data.frameio);
  const security = $derived(data.security);
  const branding = $derived(data.branding);

  async function logout() {
    await fetch("/api/auth/logout", { method: "POST" });
    await goto("/login");
  }

  const flash = $derived(page.url.searchParams.get("frameio"));

  // --- Frame.io connection ---------------------------------------------------
  let disconnecting = $state(false);
  async function disconnect() {
    if (!confirm("Disconnect this Frame.io account?")) return;
    disconnecting = true;
    try {
      await fetch("/api/frameio/disconnect", { method: "POST" });
      await goto("/settings", { replaceState: true });
      await invalidateAll();
    } finally {
      disconnecting = false;
    }
  }
  function fmt(iso: string | null | undefined): string {
    return iso ? new Date(iso).toLocaleString() : "—";
  }

  // --- SMTP test -------------------------------------------------------------
  let testing = $state(false);
  let result = $state<{ ok: boolean; msg: string } | null>(null);
  async function sendTest() {
    testing = true;
    result = null;
    try {
      const res = await fetch("/api/settings/email/test", { method: "POST" });
      if (res.ok) {
        result = { ok: true, msg: `Test email sent to ${email.notify_email}.` };
      } else {
        const body = await res.json().catch(() => ({}));
        result = { ok: false, msg: body.detail ?? `Failed (${res.status})` };
      }
    } finally {
      testing = false;
    }
  }

  // --- Password --------------------------------------------------------------
  let currentPassword = $state("");
  let newPassword = $state("");
  let confirmPassword = $state("");
  let savingPw = $state(false);
  let pwResult = $state<{ ok: boolean; msg: string } | null>(null);

  async function changePassword() {
    pwResult = null;
    if (newPassword.length < 8) {
      pwResult = { ok: false, msg: "New password must be at least 8 characters." };
      return;
    }
    if (newPassword !== confirmPassword) {
      pwResult = { ok: false, msg: "New passwords don't match." };
      return;
    }
    savingPw = true;
    try {
      const res = await fetch("/api/settings/password", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
      });
      if (res.ok) {
        pwResult = { ok: true, msg: "Password changed." };
        currentPassword = newPassword = confirmPassword = "";
      } else {
        const body = await res.json().catch(() => ({}));
        pwResult = { ok: false, msg: body.detail ?? `Failed (${res.status})` };
      }
    } finally {
      savingPw = false;
    }
  }

  // --- Passkeys --------------------------------------------------------------
  let pkBusy = $state(false);
  let pkError = $state<string | null>(null);
  async function addPasskey() {
    const name = prompt("Name this passkey (e.g. 'MacBook Touch ID'):", "Passkey");
    if (name === null) return;
    pkError = null;
    pkBusy = true;
    try {
      await registerPasskey(name || "Passkey");
      await invalidateAll();
    } catch (e) {
      pkError = e instanceof Error ? e.message : "Could not add passkey";
    } finally {
      pkBusy = false;
    }
  }
  async function removePasskey(id: string) {
    if (!confirm("Remove this passkey?")) return;
    await fetch(`/api/auth/passkeys/${id}`, { method: "DELETE" });
    await invalidateAll();
  }

  // --- TOTP ------------------------------------------------------------------
  let totpOpen = $state(false);
  let totpStep = $state<"qr" | "codes">("qr");
  let totpData = $state<{ secret: string; otpauth_uri: string; qr_svg: string } | null>(null);
  let totpCode = $state("");
  let totpError = $state<string | null>(null);
  let recoveryCodes = $state<string[]>([]);
  let totpBusy = $state(false);

  async function startTotp() {
    totpError = null;
    totpCode = "";
    recoveryCodes = [];
    totpStep = "qr";
    totpBusy = true;
    try {
      const res = await fetch("/api/settings/totp/begin", { method: "POST" });
      if (!res.ok) {
        totpError = (await res.json().catch(() => ({}))).detail ?? "Failed to start setup";
        return;
      }
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
        totpError = (await res.json().catch(() => ({}))).detail ?? "Invalid code";
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
    const pw = prompt("Enter your current password to disable two-factor:");
    if (!pw) return;
    const res = await fetch("/api/settings/totp/disable", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ password: pw }),
    });
    if (!res.ok) {
      alert((await res.json().catch(() => ({}))).detail ?? "Could not disable");
      return;
    }
    await invalidateAll();
  }

  // --- Branding --------------------------------------------------------------
  let logoBusy = $state(false);
  let logoError = $state<string | null>(null);
  let logoVersion = $state(0); // cache-busts the preview after an upload/remove
  const logoSrc = $derived(`/api/branding/logo?v=${branding.logo_updated_at ?? ""}${logoVersion}`);

  async function uploadLogo(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    logoError = null;
    logoBusy = true;
    try {
      const fd = new FormData();
      fd.append("file", file);
      const res = await fetch("/api/settings/branding/logo", { method: "POST", body: fd });
      if (!res.ok) {
        logoError = (await res.json().catch(() => ({}))).detail ?? "Upload failed";
        return;
      }
      logoVersion = Date.now();
      await invalidateAll();
    } finally {
      logoBusy = false;
      input.value = "";
    }
  }
  async function removeLogo() {
    if (!confirm("Remove the logo?")) return;
    await fetch("/api/settings/branding/logo", { method: "DELETE" });
    logoVersion = Date.now();
    await invalidateAll();
  }

  const fieldCls =
    "rounded-md border border-border bg-surface-2 px-3 py-2 text-sm placeholder:text-faint";
</script>

<div class="mx-auto max-w-2xl space-y-6">
  <PageHeader title="Settings" subtitle="Connection, email, account, and general configuration." />

  {#if flash === "connected"}
    <p class="rounded-md bg-success/10 px-3 py-2 text-sm text-success">Frame.io connected.</p>
  {:else if flash === "error"}
    <p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">
      Frame.io connection failed. Please try again.
    </p>
  {/if}

  <!-- Frame.io connection -->
  <Card class="space-y-4" >
    <div class="flex items-center justify-between">
      <h2 class="font-medium">Frame.io connection</h2>
      <StatusPill status={frameio.connected ? "connected" : "revoked"} label={frameio.connected ? "Connected" : "Not connected"} />
    </div>

    {#if frameio.connected}
      <dl class="space-y-2 text-sm">
        <div class="flex justify-between"><dt class="text-muted">Account</dt><dd>{frameio.adobe_email ?? "—"}</dd></div>
        <div class="flex justify-between"><dt class="text-muted">Token expires</dt><dd>{fmt(frameio.expires_at)}</dd></div>
        <div class="flex justify-between"><dt class="text-muted">Token health</dt><dd>{frameio.needs_refresh ? "Refresh due" : "Healthy"}</dd></div>
      </dl>
      <Button variant="ghost" size="sm" onclick={disconnect} disabled={disconnecting}>
        {disconnecting ? "Disconnecting…" : "Disconnect"}
      </Button>
    {:else}
      <p class="text-sm text-muted">Connect your Frame.io account to create destinations and sync rules.</p>
      <a href="/api/frameio/oauth/connect" data-sveltekit-reload class="inline-flex items-center justify-center gap-1.5 rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover">
        Connect Frame.io
      </a>
    {/if}
  </Card>

  <!-- Email -->
  <Card class="space-y-4">
    <div class="flex items-center justify-between">
      <h2 class="font-medium">Email (SMTP)</h2>
      <StatusPill status={email.configured ? "connected" : "revoked"} label={email.configured ? "Configured" : "Not configured"} />
    </div>

    {#if email.configured}
      <dl class="space-y-2 text-sm">
        <div class="flex justify-between"><dt class="text-muted">SMTP host</dt><dd>{email.smtp_host}</dd></div>
        <div class="flex justify-between"><dt class="text-muted">From</dt><dd>{email.smtp_from}</dd></div>
        <div class="flex justify-between"><dt class="text-muted">Notifications to</dt><dd>{email.notify_email}</dd></div>
      </dl>
      <div class="flex items-center gap-3">
        <Button size="sm" onclick={sendTest} disabled={testing}>{testing ? "Sending…" : "Send test email"}</Button>
        {#if result}<span class="text-sm {result.ok ? 'text-success' : 'text-danger'}">{result.msg}</span>{/if}
      </div>
    {:else}
      <p class="text-sm text-muted">
        Set <code class="rounded bg-surface-2 px-1">SMTP_HOST</code>, <code class="rounded bg-surface-2 px-1">SMTP_FROM</code>
        (and credentials) in <code class="rounded bg-surface-2 px-1">.env</code>, then restart Portal.
      </p>
    {/if}
  </Card>

  <!-- Password -->
  <Card class="space-y-4">
    <h2 class="font-medium">Change password</h2>
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
      <input type="password" bind:value={currentPassword} placeholder="Current password" autocomplete="current-password" class={fieldCls} />
      <input type="password" bind:value={newPassword} placeholder="New password" autocomplete="new-password" class={fieldCls} />
      <input type="password" bind:value={confirmPassword} placeholder="Confirm new password" autocomplete="new-password" class={fieldCls} />
    </div>
    <div class="flex items-center gap-3">
      <Button size="sm" onclick={changePassword} disabled={savingPw || !currentPassword || !newPassword}>
        {savingPw ? "Saving…" : "Change password"}
      </Button>
      {#if pwResult}<span class="text-sm {pwResult.ok ? 'text-success' : 'text-danger'}">{pwResult.msg}</span>{/if}
    </div>
  </Card>

  <!-- Security: passkeys + TOTP -->
  <Card class="space-y-5">
    <h2 class="font-medium">Security</h2>

    <!-- Passkeys -->
    <div class="space-y-2">
      <div class="flex items-center justify-between">
        <span class="text-sm font-medium">Passkeys</span>
        <Button size="sm" variant="ghost" onclick={addPasskey} disabled={pkBusy}>
          {pkBusy ? "Waiting…" : "＋ Add passkey"}
        </Button>
      </div>
      <p class="text-xs text-faint">Sign in without a password, using Touch ID, Windows Hello, or a security key.</p>
      {#if pkError}<p class="text-xs text-danger">{pkError}</p>{/if}
      {#if security.passkeys.length === 0}
        <p class="text-sm text-muted">No passkeys yet.</p>
      {:else}
        <ul class="divide-y divide-border/60 rounded-md border border-border">
          {#each security.passkeys as pk (pk.id)}
            <li class="flex items-center justify-between px-3 py-2 text-sm">
              <div>
                <div class="font-medium">{pk.name ?? "Passkey"}</div>
                <div class="text-xs text-faint">
                  Added {new Date(pk.created_at).toLocaleDateString()}
                  {#if pk.last_used_at}· last used {new Date(pk.last_used_at).toLocaleDateString()}{/if}
                </div>
              </div>
              <button onclick={() => removePasskey(pk.id)} class="text-xs text-faint hover:text-danger">Remove</button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <!-- TOTP -->
    <div class="space-y-2 border-t border-border pt-4">
      <div class="flex items-center justify-between">
        <span class="text-sm font-medium">Two-factor (authenticator app)</span>
        <StatusPill status={security.totp_enabled ? "connected" : "revoked"} label={security.totp_enabled ? "On" : "Off"} />
      </div>
      <p class="text-xs text-faint">A 6-digit code from an app like 1Password or Authy, required after your password.</p>
      {#if security.totp_enabled}
        <Button size="sm" variant="ghost" onclick={disableTotp}>Disable two-factor</Button>
      {:else}
        <Button size="sm" onclick={startTotp} disabled={totpBusy}>{totpBusy ? "…" : "Enable two-factor"}</Button>
        {#if totpError && !totpOpen}<span class="ml-2 text-sm text-danger">{totpError}</span>{/if}
      {/if}
    </div>
  </Card>

  <!-- Branding -->
  <Card class="space-y-4">
    <h2 class="font-medium">Branding</h2>
    <p class="text-xs text-faint">A PNG logo shown on your upload &amp; download link pages and the sign-in screen.</p>
    <div class="flex items-center gap-4">
      <div class="flex h-16 w-32 items-center justify-center rounded-md border border-border bg-surface-2">
        {#if branding.has_logo}
          <img src={logoSrc} alt="Current logo" class="max-h-12 max-w-28 object-contain" />
        {:else}
          <span class="text-xs text-faint">No logo</span>
        {/if}
      </div>
      <div class="space-y-2">
        <label class="inline-flex cursor-pointer items-center rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-2">
          {logoBusy ? "Uploading…" : "Upload .png"}
          <input type="file" accept="image/png,.png" class="hidden" onchange={uploadLogo} disabled={logoBusy} />
        </label>
        {#if branding.has_logo}
          <button onclick={removeLogo} class="ml-2 text-xs text-faint hover:text-danger">Remove</button>
        {/if}
        {#if logoError}<p class="text-xs text-danger">{logoError}</p>{/if}
      </div>
    </div>
  </Card>

  <!-- General -->
  <Card class="space-y-2 text-sm">
    <h2 class="font-medium">General</h2>
    <div class="flex items-center justify-between">
      <span class="text-muted">Base URL</span>
      <span class="font-mono text-xs">{data.general.base_url}</span>
    </div>
    <p class="text-xs text-faint">Set via <code class="rounded bg-surface-2 px-1">BASE_URL</code> in <code class="rounded bg-surface-2 px-1">.env</code>. Branding is per destination and per link.</p>
  </Card>

  <!-- Session -->
  <Card class="space-y-3">
    <h2 class="font-medium">Session</h2>
    <div class="flex items-center justify-between text-sm">
      <span class="text-muted">Signed in as</span>
      <span class="font-mono text-xs">{data.user.email}</span>
    </div>
    <Button variant="ghost" size="sm" onclick={logout}>Sign out</Button>
  </Card>
</div>

<Modal bind:open={totpOpen} title="Set up two-factor">
  {#if totpStep === "qr" && totpData}
    <div class="space-y-3 text-sm">
      <p class="text-muted">Scan this with your authenticator app, then enter the 6-digit code to confirm.</p>
      <div class="mx-auto flex w-40 justify-center rounded-md bg-white p-3">
        {@html totpData.qr_svg}
      </div>
      <p class="break-all text-xs text-faint">Or enter this key manually: <span class="font-mono text-text">{totpData.secret}</span></p>
      <input bind:value={totpCode} inputmode="numeric" autocomplete="one-time-code" placeholder="123456" class="w-full rounded-md border border-border bg-surface-2 px-3 py-2 text-center text-lg tracking-widest" />
      {#if totpError}<p class="text-sm text-danger">{totpError}</p>{/if}
    </div>
  {:else if totpStep === "codes"}
    <div class="space-y-3 text-sm">
      <p class="text-muted">Two-factor is on. Save these recovery codes somewhere safe — each works once if you lose your authenticator.</p>
      <ul class="grid grid-cols-2 gap-1 rounded-md border border-border bg-surface-2 p-3 font-mono text-xs">
        {#each recoveryCodes as c}<li>{c}</li>{/each}
      </ul>
    </div>
  {/if}
  {#snippet footer()}
    {#if totpStep === "qr"}
      <Button variant="ghost" size="sm" onclick={() => (totpOpen = false)} disabled={totpBusy}>Cancel</Button>
      <Button size="sm" onclick={confirmTotp} disabled={totpBusy || totpCode.length < 6}>{totpBusy ? "Verifying…" : "Confirm"}</Button>
    {:else}
      <Button size="sm" onclick={() => (totpOpen = false)}>Done</Button>
    {/if}
  {/snippet}
</Modal>
