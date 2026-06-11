<script lang="ts">
  let { data } = $props();
  const email = $derived(data.email);

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
</script>

<div class="max-w-2xl space-y-6">
  <div>
    <h1 class="text-2xl font-semibold">Settings</h1>
    <p class="text-sm text-neutral-500">Email and notification configuration.</p>
  </div>

  <div class="space-y-4 rounded-xl border border-neutral-200 bg-white p-6">
    <div class="flex items-center justify-between">
      <h2 class="font-medium">Email (SMTP)</h2>
      <span class="rounded-full px-2.5 py-0.5 text-xs font-medium {email.configured ? 'bg-green-100 text-green-800' : 'bg-neutral-100 text-neutral-600'}">
        {email.configured ? "Configured" : "Not configured"}
      </span>
    </div>

    {#if email.configured}
      <dl class="space-y-2 text-sm">
        <div class="flex justify-between"><dt class="text-neutral-500">SMTP host</dt><dd>{email.smtp_host}</dd></div>
        <div class="flex justify-between"><dt class="text-neutral-500">From</dt><dd>{email.smtp_from}</dd></div>
        <div class="flex justify-between"><dt class="text-neutral-500">Notifications to</dt><dd>{email.notify_email}</dd></div>
      </dl>
      <div class="flex items-center gap-3">
        <button onclick={sendTest} disabled={testing} class="rounded-md bg-neutral-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-neutral-800 disabled:opacity-50">
          {testing ? "Sending…" : "Send test email"}
        </button>
        {#if result}
          <span class="text-sm {result.ok ? 'text-green-700' : 'text-red-700'}">{result.msg}</span>
        {/if}
      </div>
    {:else}
      <p class="text-sm text-neutral-500">
        Set <code class="rounded bg-neutral-100 px-1">SMTP_HOST</code>, <code class="rounded bg-neutral-100 px-1">SMTP_FROM</code>
        (and credentials) in <code class="rounded bg-neutral-100 px-1">.env</code>, then restart Portal. Notifications are
        sent on upload completion.
      </p>
    {/if}
  </div>

  <div class="space-y-4 rounded-xl border border-neutral-200 bg-white p-6">
    <h2 class="font-medium">Change password</h2>
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
      <input type="password" bind:value={currentPassword} placeholder="Current password" autocomplete="current-password" class="rounded-md border border-neutral-300 px-3 py-2 text-sm" />
      <input type="password" bind:value={newPassword} placeholder="New password" autocomplete="new-password" class="rounded-md border border-neutral-300 px-3 py-2 text-sm" />
      <input type="password" bind:value={confirmPassword} placeholder="Confirm new password" autocomplete="new-password" class="rounded-md border border-neutral-300 px-3 py-2 text-sm" />
    </div>
    <div class="flex items-center gap-3">
      <button onclick={changePassword} disabled={savingPw || !currentPassword || !newPassword} class="rounded-md bg-neutral-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-neutral-800 disabled:opacity-50">
        {savingPw ? "Saving…" : "Change password"}
      </button>
      {#if pwResult}
        <span class="text-sm {pwResult.ok ? 'text-green-700' : 'text-red-700'}">{pwResult.msg}</span>
      {/if}
    </div>
  </div>

  <div class="rounded-xl border border-neutral-200 bg-white p-6 text-sm">
    <h2 class="mb-3 font-medium">General</h2>
    <div class="flex items-center justify-between">
      <span class="text-neutral-500">Base URL</span>
      <span class="font-mono text-xs">{data.general.base_url}</span>
    </div>
    <p class="mt-2 text-xs text-neutral-400">Base URL is set via <code class="rounded bg-neutral-100 px-1">BASE_URL</code> in <code class="rounded bg-neutral-100 px-1">.env</code>. Branding is configured per destination and per link.</p>
  </div>
</div>
