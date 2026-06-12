<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";

  let { data, children } = $props();

  async function logout() {
    await fetch("/api/auth/logout", { method: "POST" });
    await goto("/login");
  }

  // Each section owns one or more route prefixes; the sidebar highlights by prefix
  // so e.g. both /upload-links and /download-links light up "Links".
  type Section = { label: string; href: string; match: string[]; icon: string };
  const sections: Section[] = [
    {
      label: "Dashboard",
      href: "/",
      match: ["/activity"],
      icon: "M3 12 12 3l9 9M5 10v10h5v-6h4v6h5V10",
    },
    {
      label: "Files",
      href: "/explorer",
      match: ["/explorer"],
      icon: "M4 7a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z",
    },
    {
      label: "Links",
      href: "/upload-links",
      match: ["/upload-links", "/download-links"],
      icon: "M9 15l6-6M10 6l1-1a4 4 0 0 1 6 6l-1 1M14 18l-1 1a4 4 0 0 1-6-6l1-1",
    },
    {
      label: "Sync",
      href: "/destinations",
      match: ["/destinations", "/sync-rules"],
      icon: "M4 8a8 8 0 0 1 14-3l2 2M20 16a8 8 0 0 1-14 3l-2-2M18 4v3h-3M6 20v-3h3",
    },
    {
      label: "Settings",
      href: "/settings",
      match: ["/settings", "/connections"],
      icon: "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6M19 12a7 7 0 0 0-.1-1l2-1.5-2-3.4-2.3 1a7 7 0 0 0-1.7-1l-.3-2.6H9.4l-.3 2.6a7 7 0 0 0-1.7 1l-2.3-1-2 3.4 2 1.5a7 7 0 0 0 0 2l-2 1.5 2 3.4 2.3-1a7 7 0 0 0 1.7 1l.3 2.6h5.2l.3-2.6a7 7 0 0 0 1.7-1l2.3 1 2-3.4-2-1.5c.06-.32.1-.66.1-1Z",
    },
  ];

  function active(s: Section): boolean {
    const p = page.url.pathname;
    if (s.href === "/") return p === "/" || s.match.some((m) => p.startsWith(m));
    return p === s.href || [s.href, ...s.match].some((m) => p.startsWith(m + "/") || p === m);
  }
</script>

<div class="flex min-h-full">
  <aside
    class="sticky top-0 flex h-screen w-56 shrink-0 flex-col border-r border-border bg-surface px-3 py-4"
  >
    <a href="/" class="mb-6 flex items-center gap-2 px-2">
      <span class="grid h-7 w-7 place-items-center rounded-md bg-accent text-on-accent font-bold">P</span>
      <span class="text-base font-semibold tracking-tight">Portal</span>
    </a>

    <nav class="flex flex-1 flex-col gap-1">
      {#each sections as s (s.label)}
        <a
          href={s.href}
          class="flex items-center gap-3 rounded-md px-2.5 py-2 text-sm font-medium transition-colors {active(
            s,
          )
            ? 'bg-surface-3 text-text'
            : 'text-muted hover:bg-surface-2 hover:text-text'}"
        >
          <svg
            class="h-[18px] w-[18px] {active(s) ? 'text-accent' : ''}"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.7"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path d={s.icon} />
          </svg>
          {s.label}
        </a>
      {/each}
    </nav>

    <div class="mt-4 border-t border-border pt-3">
      <div class="truncate px-2.5 text-xs text-faint" title={data.user.email}>{data.user.email}</div>
      <button
        onclick={logout}
        class="mt-1.5 w-full rounded-md px-2.5 py-1.5 text-left text-sm text-muted hover:bg-surface-2 hover:text-text"
      >
        Sign out
      </button>
    </div>
  </aside>

  <main class="min-w-0 flex-1 px-8 py-7">
    {@render children()}
  </main>
</div>
