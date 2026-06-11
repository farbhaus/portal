<script lang="ts">
  import { page } from "$app/state";

  // Segmented sub-navigation. Active tab is whichever href the current path
  // starts with (so /upload-links/new still highlights "Upload").
  let { tabs }: { tabs: { label: string; href: string }[] } = $props();

  function isActive(href: string): boolean {
    return page.url.pathname === href || page.url.pathname.startsWith(href + "/");
  }
</script>

<nav class="inline-flex rounded-lg border border-border bg-surface p-1 text-sm">
  {#each tabs as tab (tab.href)}
    <a
      href={tab.href}
      class="rounded-md px-3 py-1.5 font-medium transition-colors {isActive(tab.href)
        ? 'bg-surface-3 text-text'
        : 'text-muted hover:text-text'}"
    >
      {tab.label}
    </a>
  {/each}
</nav>
