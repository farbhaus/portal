<script lang="ts">
  import { goto } from "$app/navigation";

  let email = $state("");
  let password = $state("");
  let error = $state<string | null>(null);
  let loading = $state(false);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    loading = true;
    try {
      // Same-origin via Caddy; the API sets the session cookie directly in the browser.
      const res = await fetch("/api/auth/login", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ email, password }),
      });
      if (res.ok) {
        await goto("/");
      } else {
        const body = await res.json().catch(() => ({}));
        error = body.detail ?? "Login failed";
      }
    } catch {
      error = "Network error";
    } finally {
      loading = false;
    }
  }
</script>

<div class="flex min-h-full items-center justify-center p-6">
  <form
    onsubmit={submit}
    class="w-full max-w-sm space-y-5 rounded-xl border border-border bg-surface p-8 shadow-sm"
  >
    <div class="space-y-1">
      <h1 class="text-xl font-semibold">Portal</h1>
      <p class="text-sm text-muted">Sign in to the admin dashboard.</p>
    </div>

    {#if error}
      <p class="rounded-md bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>
    {/if}

    <div class="space-y-2">
      <label class="block text-sm font-medium" for="email">Email</label>
      <input
        id="email"
        type="email"
        bind:value={email}
        required
        autocomplete="username"
        class="w-full rounded-md border border-border px-3 py-2 text-sm outline-none focus:border-accent"
      />
    </div>

    <div class="space-y-2">
      <label class="block text-sm font-medium" for="password">Password</label>
      <input
        id="password"
        type="password"
        bind:value={password}
        required
        autocomplete="current-password"
        class="w-full rounded-md border border-border px-3 py-2 text-sm outline-none focus:border-accent"
      />
    </div>

    <button
      type="submit"
      disabled={loading}
      class="w-full rounded-md bg-accent px-3 py-2 text-sm font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50"
    >
      {loading ? "Signing in…" : "Sign in"}
    </button>
  </form>
</div>
