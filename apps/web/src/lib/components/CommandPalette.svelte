<script lang="ts">
  // ⌘K / Ctrl+K anywhere in the admin: start something, jump to a link, or find a
  // Frame.io folder to receive into. Links and rules load when it opens; folder search kicks in
  // from two letters, through the same search the picker uses.
  import { goto } from "$app/navigation";
  import { tick } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import { crumbNames, listAccounts, search, type SearchHit } from "$lib/frameio";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  type Item = { id: string; group: string; label: string; hint?: string; icon: IconName; run: () => void };

  let q = $state("");
  let sel = $state(0);
  let input = $state<HTMLInputElement | null>(null);
  let list = $state<HTMLDivElement | null>(null);
  let links = $state<{ id: string; kind: string; name: string; folder?: unknown; source?: { path?: { id: string; name: string }[] } }[]>([]);
  let hits = $state<SearchHit[]>([]);
  let account = "";
  let searchSeq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const go = (href: string) => () => {
    open = false;
    void goto(href);
  };

  const actions: Item[] = [
    { id: "a-recv", group: "Create", label: "New receive link", hint: "Clients upload into Frame.io", icon: "receive", run: go("/links/new") },
    { id: "a-send", group: "Create", label: "New send link", hint: "Clients download from Frame.io", icon: "send", run: go("/links/new?type=download&pick=1") },
    { id: "g-dash", group: "Go to", label: "Dashboard", icon: "home", run: go("/") },
    { id: "g-hist", group: "Go to", label: "History", icon: "history", run: go("/#history") },
    { id: "g-links", group: "Go to", label: "Links", icon: "link", run: go("/links") },
    { id: "g-set", group: "Go to", label: "Settings", icon: "sliders", run: go("/settings") },
    { id: "g-email", group: "Go to", label: "Settings · Email", icon: "mail", run: go("/settings#email") },
    { id: "g-brand", group: "Go to", label: "Settings · Branding", icon: "sliders", run: go("/settings#branding") },
    { id: "g-presets", group: "Go to", label: "Settings · Link presets", icon: "sliders", run: go("/settings#presets") },
    { id: "g-frame", group: "Go to", label: "Settings · Frame.io", icon: "sliders", run: go("/settings#frameio") },
    { id: "g-log", group: "Go to", label: "Admin log", icon: "history", run: go("/activity") },
  ];

  $effect(() => {
    if (!open) return;
    q = "";
    sel = 0;
    hits = [];
    void (async () => {
      await tick();
      input?.focus();
      links = await fetch("/api/links").then((x) => (x.ok ? x.json() : [])).catch(() => []);
      account = (await listAccounts().catch(() => []))[0]?.id ?? "";
    })();
  });

  function onInput() {
    sel = 0;
    clearTimeout(timer);
    hits = [];
    const s = q.trim();
    if (s.length < 2 || !account) return;
    const my = ++searchSeq;
    timer = setTimeout(async () => {
      const r = await search(account, s).catch(() => []);
      if (my === searchSeq) hits = r.slice(0, 6);
    }, 250);
  }

  const items = $derived.by((): Item[] => {
    const s = q.trim().toLowerCase();
    const has = (...xs: (string | undefined | null)[]) => !s || xs.some((x) => x?.toLowerCase().includes(s));
    const out: Item[] = actions.filter((a) => has(a.label, a.hint));
    if (s) {
      for (const l of links) {
        const where = l.kind === "upload" ? crumbNames(l.folder as never) : crumbNames({ account_id: "", folder_id: "", path: l.source?.path ?? [] });
        if (!has(l.name, where.join(" "))) continue;
        out.push({
          id: `l-${l.id}`,
          group: "Links",
          label: l.name,
          hint: where.join(" / "),
          icon: l.kind === "upload" ? "receive" : "send",
          run: go(l.kind === "upload" ? `/upload-links/${l.id}` : `/download-links/${l.id}`),
        });
        if (out.filter((x) => x.group === "Links").length >= 6) break;
      }
      for (const h of hits) {
        const folder = h.kind === "project" ? h.root_folder_id : h.id;
        if (!folder) continue;
        out.push({
          id: `f-${h.id}`,
          group: "Receive into a Frame.io folder",
          label: h.name,
          hint: h.kind === "project" ? "Project" : (h.where ?? ""),
          icon: h.kind === "project" ? "project" : "folder",
          run: go(`/links/new?account_id=${encodeURIComponent(account)}&folder_id=${encodeURIComponent(folder)}`),
        });
      }
    }
    return out;
  });

  const groups = $derived.by(() => {
    const m = new Map<string, { item: Item; i: number }[]>();
    items.forEach((item, i) => {
      if (!m.has(item.group)) m.set(item.group, []);
      m.get(item.group)!.push({ item, i });
    });
    return [...m.entries()];
  });

  function keep(i: number) {
    sel = Math.max(0, Math.min(items.length - 1, i));
    queueMicrotask(() => list?.querySelector(`[data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" }));
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      keep(sel + 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      keep(sel - 1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      items[sel]?.run();
    } else if (e.key === "Escape") {
      e.preventDefault();
      open = false;
    }
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      open = !open;
    }
  }}
/>

{#if open}
  <div
    class="fixed inset-0 z-50 flex items-start justify-center bg-black/60 px-4 pt-[12vh]"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) open = false;
    }}
  >
    <div class="flex max-h-[70vh] w-full max-w-[600px] flex-col overflow-hidden rounded-xl border border-border-strong bg-surface text-[13px] shadow-2xl" role="dialog" aria-modal="true" aria-label="Command palette">
      <label class="flex items-center gap-2.5 border-b border-border px-4">
        <span class="text-faint"><Icon name="search" /></span>
        <input
          bind:this={input}
          bind:value={q}
          oninput={onInput}
          onkeydown={onKey}
          class="h-12 min-w-0 flex-1 bg-transparent text-sm text-text outline-none placeholder:text-faint"
          placeholder="Search links and folders"
          aria-label="Search"
          autocomplete="off"
          spellcheck="false"
        />
        <kbd class="rounded border border-border px-1.5 py-0.5 font-mono text-[10.5px] text-faint">esc</kbd>
      </label>
      <div bind:this={list} class="min-h-0 overflow-y-auto p-1.5" role="listbox" aria-label="Results">
        {#each groups as [group, rows] (group)}
          <div class="px-2.5 pt-2.5 pb-1 text-[11.5px] font-medium text-faint">{group}</div>
          {#each rows as { item, i } (item.id)}
            <div
              data-i={i}
              role="option"
              tabindex="-1"
              aria-selected={i === sel}
              class="flex h-9 cursor-pointer items-center gap-2.5 rounded-md px-2.5 {i === sel ? 'bg-surface-3 text-text' : 'text-body hover:bg-surface-2'}"
              onclick={item.run}
              onkeydown={() => {}}
              onmousemove={() => (sel = i)}
            >
              <span class="text-faint"><Icon name={item.icon} /></span>
              <span class="min-w-0 flex-1 truncate">{item.label}</span>
              {#if item.hint}<span class="max-w-[45%] truncate text-xs text-faint">{item.hint}</span>{/if}
            </div>
          {/each}
        {:else}
          <p class="px-3 py-8 text-center text-faint">Nothing matches “{q}”.</p>
        {/each}
      </div>
    </div>
  </div>
{/if}
