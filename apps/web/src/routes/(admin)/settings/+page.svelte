<script lang="ts">
  import { goto, invalidateAll } from "$app/navigation";
  import { page } from "$app/state";
  import { Card, Button, StatusPill, PageHeader } from "$lib/components";

  let { data } = $props();
  const email = $derived(data.email);
  const frameio = $derived(data.frameio);

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
