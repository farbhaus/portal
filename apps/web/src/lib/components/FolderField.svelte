<script lang="ts">
  // A Frame.io folder shown as a breadcrumb you can click: each segment opens the picker at that
  // level, the project icon opens the project list, and the rest opens the picker in the folder.
  import FrameioPicker from "./FrameioPicker.svelte";
  import Icon from "./Icon.svelte";
  import { crumbNames, type FolderRef } from "$lib/frameio";

  let {
    value = $bindable(null),
    recents = [],
    title = "Choose a folder",
    placeholder = "Choose a Frame.io folder",
    chooseVerb = "Choose",
    startAt = null,
    id = undefined,
    onchange = undefined,
  }: {
    value?: FolderRef | null;
    recents?: FolderRef[];
    title?: string;
    placeholder?: string;
    chooseVerb?: string;
    /** Where the picker opens while nothing is chosen (default: the recent folders). */
    startAt?: FolderRef | null;
    id?: string;
    onchange?: (f: FolderRef) => void;
  } = $props();

  let open = $state(false);
  let start = $state<FolderRef | null>(null);
  let atProjects = $state(false);

  const names = $derived(crumbNames(value));

  /** Open the picker; `k` is the clicked breadcrumb segment (-1 = the project list). */
  function openAt(k: number | null) {
    atProjects = k === -1;
    if (!value) {
      start = startAt;
    } else if (k === null || k < 0 || !value.path?.length || k >= value.path.length - 1) {
      start = value;
    } else {
      const path = value.path.slice(0, k + 1);
      start = { ...value, path, folder_id: path[path.length - 1].id, folder_name: path[path.length - 1].name };
    }
    open = true;
  }

  function onClick(e: MouseEvent) {
    const seg = (e.target as HTMLElement).closest<HTMLElement>("[data-seg]");
    openAt(seg ? Number(seg.dataset.seg) : null);
  }
</script>

<button
  type="button"
  {id}
  onclick={onClick}
  class="group flex h-8 w-full min-w-0 items-center gap-1 rounded-md border border-border bg-bg pr-2 pl-1 text-left text-[13px] text-text hover:border-border-strong focus-visible:border-accent"
>
  {#if value}
    <span data-seg="-1" title="All projects" class="grid place-items-center rounded p-1 text-faint hover:bg-surface-3 hover:text-text"><Icon name="project" /></span>
    <span class="min-w-0 flex-1 truncate">
      {#each names as n, k (k)}
        {#if k > 0}<span class="px-px text-faint">/</span>{/if}<span
          data-seg={k}
          class="rounded px-1 py-0.5 hover:bg-surface-3 hover:text-text {k === names.length - 1 ? 'text-text' : 'text-muted'}"
        >{n}</span>
      {/each}
    </span>
  {:else}
    <span class="grid place-items-center p-1 text-faint"><Icon name="folder" /></span>
    <span class="flex-1 truncate text-faint">{placeholder}</span>
  {/if}
  <span class="text-faint group-hover:text-muted"><Icon name="chevron" size={14} /></span>
</button>

<FrameioPicker
  bind:open
  {title}
  {start}
  startAtProjects={atProjects}
  {recents}
  {chooseVerb}
  onchoose={(f) => {
    value = f;
    onchange?.(f);
  }}
/>
