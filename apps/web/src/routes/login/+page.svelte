<script lang="ts">
  import { goto } from "$app/navigation";
  import { PoweredByPortal } from "$lib/components";
  import { loginWithPasskey } from "$lib/webauthn";

  let email = $state("");
  let password = $state("");
  let code = $state("");
  let totpRequired = $state(false);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let logoOk = $state(true);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    loading = true;
    try {
      const res = await fetch("/api/auth/login", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ email, password }),
      });
      if (!res.ok) {
        error = (await res.json().catch(() => ({}))).detail ?? "Login failed";
        return;
      }
      const body = await res.json();
      if (body.totp_required) {
        totpRequired = true;
      } else {
        await goto("/");
      }
    } catch {
      error = "Network error";
    } finally {
      loading = false;
    }
  }

  async function submitTotp(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    loading = true;
    try {
      const res = await fetch("/api/auth/login/totp", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ code }),
      });
      if (res.ok) {
        await goto("/");
      } else {
        error = (await res.json().catch(() => ({}))).detail ?? "Invalid code";
      }
    } finally {
      loading = false;
    }
  }

  async function passkey() {
    error = null;
    loading = true;
    try {
      await loginWithPasskey();
      await goto("/");
    } catch (err) {
      error = err instanceof Error ? err.message : "Passkey sign-in failed";
    } finally {
      loading = false;
    }
  }
</script>

<div class="flex min-h-full items-center justify-center p-6">
  <div class="w-full max-w-sm">
    <div class="space-y-5 rounded-xl border border-border bg-surface p-8 shadow-sm">
    <div class="space-y-1 text-center">
      {#if logoOk}
        <img src="/api/branding/logo" alt="" class="mx-auto mb-2 h-12 object-contain" onerror={() => (logoOk = false)} />
      {/if}
      <h1 class="text-xl font-semibold">Portal</h1>
      <p class="text-sm text-muted">Sign in to the admin dashboard.</p>
    </div>

    {#if error}
      <p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
    {/if}

    {#if totpRequired}
      <form onsubmit={submitTotp} class="space-y-4">
        <p class="text-sm text-muted">Enter the 6-digit code from your authenticator app (or a recovery code).</p>
        <input
          bind:value={code}
          required
          inputmode="numeric"
          autocomplete="one-time-code"
          placeholder="123456"
          class="w-full rounded-md border border-border px-3 py-2 text-center text-lg tracking-widest outline-none focus:border-accent"
        />
        <button type="submit" disabled={loading} class="w-full rounded-md bg-accent px-3 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50">
          {loading ? "Verifying…" : "Verify"}
        </button>
        <button type="button" onclick={() => { totpRequired = false; code = ""; }} class="w-full text-xs text-faint hover:text-text">← Back</button>
      </form>
    {:else}
      <form onsubmit={submit} class="space-y-5">
        <div class="space-y-2">
          <label class="block text-sm font-medium" for="email">Email</label>
          <input id="email" type="email" bind:value={email} required autocomplete="username" class="w-full rounded-md border border-border px-3 py-2 text-sm outline-none focus:border-accent" />
        </div>
        <div class="space-y-2">
          <label class="block text-sm font-medium" for="password">Password</label>
          <input id="password" type="password" bind:value={password} required autocomplete="current-password" class="w-full rounded-md border border-border px-3 py-2 text-sm outline-none focus:border-accent" />
        </div>
        <button type="submit" disabled={loading} class="w-full rounded-md bg-accent px-3 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50">
          {loading ? "Signing in…" : "Sign in"}
        </button>
      </form>

      <div class="flex items-center gap-3 text-xs text-faint">
        <span class="h-px flex-1 bg-border"></span>or<span class="h-px flex-1 bg-border"></span>
      </div>
      <button type="button" onclick={passkey} disabled={loading} class="w-full rounded-md border border-border px-3 py-2 text-sm font-medium hover:bg-surface-2 disabled:opacity-50">
        Sign in with a passkey
      </button>
    {/if}
    </div>
    <PoweredByPortal privacy={false} />
  </div>
</div>
