<script lang="ts">
  // A path-template text field paired with a palette of clickable token chips. Clicking a token
  // inserts {name} at the caret; clicking an already-present (active) token removes it. The field
  // stays editable so admins can type separators ("/") and literal folder names.
  import type { Token } from "$lib/tokens";

  let {
    value = $bindable(""),
    tokens,
    placeholder = "",
    id,
  }: {
    value?: string;
    tokens: Token[];
    placeholder?: string;
    id?: string;
  } = $props();

  let input = $state<HTMLInputElement | null>(null);
  // Last known caret position, so a chip click inserts where the admin was typing.
  let caret = $state<number | null>(null);

  function syncCaret() {
    caret = input?.selectionStart ?? null;
  }

  function isActive(name: string): boolean {
    return value.includes(`{${name}}`);
  }

  function toggle(name: string) {
    const tok = `{${name}}`;
    if (value.includes(tok)) {
      // Remove the token along with one adjacent "/" so we don't leave a dangling separator.
      value = value.replace(`${tok}/`, "").replace(`/${tok}`, "").replace(tok, "");
      caret = null;
      return;
    }
    const at = caret ?? value.length;
    const before = value.slice(0, at);
    const after = value.slice(at);
    // Auto-insert a "/" so tokens don't run together, unless a separator is already adjacent.
    const sep = before && !before.endsWith("/") && !after.startsWith("/") ? "/" : "";
    const insert = sep + tok;
    value = before + insert + after;
    const next = at + insert.length;
    caret = next;
    // Restore focus and place the caret just past the inserted token.
    queueMicrotask(() => {
      input?.focus();
      input?.setSelectionRange(next, next);
    });
  }
</script>

<div class="mt-1">
  <input
    bind:this={input}
    bind:value
    {id}
    {placeholder}
    onselect={syncCaret}
    onkeyup={syncCaret}
    onclick={syncCaret}
    onblur={syncCaret}
    class="w-full rounded-md border border-border px-2 py-1.5 font-mono text-xs"
  />
  <div class="mt-2 flex flex-wrap gap-1.5">
    {#each tokens as t (t.name)}
      <button
        type="button"
        title={t.desc}
        onclick={() => toggle(t.name)}
        class="rounded border px-1.5 py-0.5 font-mono text-xs transition-colors {isActive(t.name)
          ? 'border-accent bg-accent text-on-accent'
          : 'border-border bg-surface-2 text-muted hover:bg-surface-3 hover:text-text'}"
      >
        {t.name}
      </button>
    {/each}
  </div>
  <p class="mt-1.5 text-xs text-faint">
    Click a token to add it; click an active one to remove it. A <span class="font-mono">/</span> is added between tokens automatically.
  </p>
</div>
