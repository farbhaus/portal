<script lang="ts">
  // Copy-to-clipboard with a brief "Copied!" confirmation. Renders the value
  // as truncating text by default; pass `label` for a fixed caption instead.
  let {
    value,
    label = undefined,
    class: klass = "",
  }: { value: string; label?: string; class?: string } = $props();

  let copied = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function copy() {
    await navigator.clipboard.writeText(value);
    copied = true;
    clearTimeout(timer);
    timer = setTimeout(() => (copied = false), 1500);
  }
</script>

<button
  onclick={copy}
  title={value}
  class="inline-flex max-w-full items-center gap-1.5 truncate text-left transition-colors {copied
    ? 'text-accent'
    : 'text-muted hover:text-text'} {klass}"
>
  <span class="truncate">{copied ? "Copied!" : (label ?? value)}</span>
</button>
