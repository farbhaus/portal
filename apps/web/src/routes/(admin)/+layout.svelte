<script lang="ts">
  import { onNavigate } from "$app/navigation";
  import { page } from "$app/state";
  import { onMount } from "svelte";
  import { CommandPalette, Icon, Toaster, type IconName } from "$lib/components";
  import { onLiveChange, startLive, stopLive } from "$lib/live.svelte";

  let { children } = $props();
  let paletteOpen = $state(false);

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

  // What the nav badges show; refreshed on every live change.
  type Nav = { links_active: number; uploads_live: number };
  let nav = $state<Nav | null>(null);
  async function loadNav() {
    try {
      const res = await fetch("/api/nav");
      if (res.ok) nav = await res.json();
    } catch {
      /* offline for a moment; the next change retries */
    }
  }
  onMount(() => {
    startLive();
    void loadNav();
    const off = onLiveChange(() => void loadNav());
    return () => {
      off();
      stopLive();
    };
  });

  type Section = { label: string; href: string; match: string[]; icon: IconName };
  const sections: Section[] = [
    { label: "Dashboard", href: "/", match: ["/activity", "/history"], icon: "home" },
    // Files (/explorer) is hidden for now: links and the picker cover what it was used for.
    { label: "Links", href: "/links", match: ["/links", "/upload-links", "/download-links"], icon: "link" },
    { label: "Settings", href: "/settings", match: ["/settings", "/connections"], icon: "sliders" },
  ];

  function active(s: Section): boolean {
    const p = page.url.pathname;
    if (s.href === "/") return p === "/" || s.match.some((m) => p.startsWith(m));
    return p === s.href || [s.href, ...s.match].some((m) => p === m || p.startsWith(m + "/"));
  }
</script>

<div class="min-h-full">
  <main class="mx-auto max-w-7xl px-4 py-7 pb-28 sm:px-6 sm:pb-7 sm:pt-24">
    {@render children()}
  </main>

  <!-- Floating pill navigation — full-width at the bottom on mobile, centered at the top on desktop -->
  <nav
    class="fixed bottom-5 left-4 right-4 z-40 flex items-center gap-0.5 rounded-full border border-border bg-surface/85 p-[5px] shadow-xl backdrop-blur-md sm:bottom-auto sm:left-1/2 sm:right-auto sm:top-5 sm:w-auto sm:-translate-x-1/2"
    aria-label="Main navigation"
  >
    {#each sections as s (s.label)}
      {@const on = active(s)}
      <a
        href={s.href}
        class="relative flex h-9 flex-1 items-center justify-center gap-2 rounded-full px-5 text-[13px] font-medium transition-colors sm:h-8 sm:flex-none {on
          ? 'bg-surface-3 text-accent'
          : 'text-muted hover:bg-surface-2 hover:text-text'}"
        aria-current={on ? "page" : undefined}
      >
        <Icon name={s.icon} size={17} />
        <span class="hidden sm:inline">{s.label}</span>
        {#if s.href === "/" && nav?.uploads_live}
          <span class="live-dot absolute top-2 right-3 sm:static" title="{nav.uploads_live} upload{nav.uploads_live === 1 ? '' : 's'} arriving now"></span>
        {:else if s.href === "/links" && nav?.links_active}
          <span class="tnum hidden h-[18px] min-w-[18px] place-items-center rounded-full px-1.5 text-[11px] font-semibold sm:grid {on ? 'bg-accent text-on-accent' : 'bg-surface-3 text-body'}" title="{nav.links_active} active link{nav.links_active === 1 ? '' : 's'}">{nav.links_active}</span>
        {/if}
      </a>
    {/each}
    <button
      type="button"
      class="flex h-9 flex-1 items-center justify-center gap-2 rounded-full px-4 text-muted transition-colors hover:bg-surface-2 hover:text-text sm:h-8 sm:flex-none sm:px-3"
      title="Search and commands (⌘K)"
      aria-label="Search and commands"
      onclick={() => (paletteOpen = true)}
    >
      <Icon name="search" size={17} />
      <kbd class="hidden font-mono text-[10.5px] text-faint sm:inline">⌘K</kbd>
    </button>
  </nav>
</div>

<CommandPalette bind:open={paletteOpen} />
<Toaster />
