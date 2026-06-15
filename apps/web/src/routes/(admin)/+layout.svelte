<script lang="ts">
  import { onNavigate } from "$app/navigation";
  import { page } from "$app/state";

  let { children } = $props();

  // View transitions API — crossfade between pages
  onNavigate((navigation) => {
    if (!document.startViewTransition) return;
    return new Promise((resolve) => {
      document.startViewTransition(async () => {
        resolve();
        await navigation.complete;
      });
    });
  });

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
      href: "/links",
      match: ["/links", "/upload-links", "/download-links"],
      icon: "M9 15l6-6M10 6l1-1a4 4 0 0 1 6 6l-1 1M14 18l-1 1a4 4 0 0 1-6-6l1-1",
    },
    {
      label: "Sync",
      href: "/sync",
      match: ["/sync", "/destinations", "/sync-rules"],
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
    return p === s.href || [s.href, ...s.match].some((m) => p === m || p.startsWith(m + "/"));
  }
</script>

<div class="min-h-full">
  <main class="mx-auto max-w-7xl px-4 py-7 pb-28 sm:px-6">
    {@render children()}
  </main>

  <!-- Floating pill navigation — stretches to device width on mobile -->
  <nav
    class="fixed bottom-5 left-4 right-4 z-50 flex items-center rounded-full border border-border bg-surface/85 px-2 py-2 shadow-xl backdrop-blur-md sm:left-1/2 sm:right-auto sm:w-auto sm:-translate-x-1/2"
    aria-label="Main navigation"
  >
    {#each sections as s (s.label)}
      <a
        href={s.href}
        class="flex flex-1 items-center justify-center gap-2 rounded-full px-4 py-2.5 text-sm font-medium transition-colors sm:flex-none {active(s)
          ? 'bg-surface-3 text-accent'
          : 'text-muted hover:bg-surface-2 hover:text-text'}"
        aria-current={active(s) ? 'page' : undefined}
      >
        <svg
          class="h-5 w-5 shrink-0"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.7"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d={s.icon} />
        </svg>
        <span class="hidden sm:inline">{s.label}</span>
      </a>
    {/each}
  </nav>
</div>
