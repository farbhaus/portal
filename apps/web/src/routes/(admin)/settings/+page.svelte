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

  <p class="text-xs text-neutral-400">
    More settings (branding defaults, base URL, password change) arrive in a later build step.
  </p>
</div>
