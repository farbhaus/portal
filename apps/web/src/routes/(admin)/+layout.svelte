<script lang="ts">
  import { goto } from "$app/navigation";
  let { data, children } = $props();

  async function logout() {
    await fetch("/api/auth/logout", { method: "POST" });
    await goto("/login");
  }
</script>

<div class="flex min-h-full flex-col">
  <header class="flex items-center justify-between border-b border-neutral-200 bg-white px-6 py-3">
    <div class="flex items-center gap-4">
      <a href="/" class="text-base font-semibold">Portal</a>
      <nav class="flex items-center gap-3 text-sm text-neutral-500">
        <a href="/" class="hover:text-neutral-900">Dashboard</a>
        <a href="/destinations" class="hover:text-neutral-900">Destinations</a>
        <a href="/upload-links" class="hover:text-neutral-900">Upload links</a>
        <a href="/connections" class="hover:text-neutral-900">Connections</a>
      </nav>
    </div>
    <div class="flex items-center gap-4 text-sm">
      <span class="text-neutral-500">{data.user.email}</span>
      <button onclick={logout} class="rounded-md border border-neutral-300 px-3 py-1.5 hover:bg-neutral-100">
        Sign out
      </button>
    </div>
  </header>
  <main class="flex-1 p-6">
    {@render children()}
  </main>
</div>
