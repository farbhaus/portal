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
    class="w-full max-w-sm space-y-5 rounded-xl border border-neutral-200 bg-white p-8 shadow-sm"
  >
    <div class="space-y-1">
      <h1 class="text-xl font-semibold">Portal</h1>
      <p class="text-sm text-neutral-500">Sign in to the admin dashboard.</p>
    </div>

    {#if error}
      <p class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">{error}</p>
    {/if}

    <div class="space-y-2">
      <label class="block text-sm font-medium" for="email">Email</label>
      <input
        id="email"
        type="email"
        bind:value={email}
        required
        autocomplete="username"
        class="w-full rounded-md border border-neutral-300 px-3 py-2 text-sm outline-none focus:border-neutral-900"
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
        class="w-full rounded-md border border-neutral-300 px-3 py-2 text-sm outline-none focus:border-neutral-900"
      />
    </div>

    <button
      type="submit"
      disabled={loading}
      class="w-full rounded-md bg-neutral-900 px-3 py-2 text-sm font-medium text-white hover:bg-neutral-800 disabled:opacity-50"
    >
      {loading ? "Signing in…" : "Sign in"}
    </button>
  </form>
</div>
