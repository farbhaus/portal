<script lang="ts">
  import type { Snippet } from "svelte";
  import { accentText } from "$lib/format";

  type Variant = "primary" | "ghost" | "danger" | "subtle";
  type Size = "sm" | "md";

  let {
    variant = "primary",
    size = "md",
    href = undefined,
    type = "button",
    disabled = false,
    target = undefined,
    rel = undefined,
    onclick = undefined,
    accent = undefined,
    class: klass = "",
    children,
  }: {
    variant?: Variant;
    size?: Size;
    href?: string;
    type?: "button" | "submit";
    disabled?: boolean;
    target?: string;
    rel?: string;
    onclick?: (e: MouseEvent) => void;
    // Optional hex colour for a primary button — used by branded public pages so a link's accent
    // drives the CTA instead of the app amber. Ignored for non-primary variants.
    accent?: string;
    class?: string;
    children: Snippet;
  } = $props();

  const base =
    "inline-flex items-center justify-center gap-1.5 rounded-md font-medium transition-colors disabled:opacity-50 disabled:pointer-events-none whitespace-nowrap";
  const sizes: Record<Size, string> = {
    sm: "px-2.5 py-1 text-xs",
    md: "px-4 py-2 text-sm",
  };
  const variants: Record<Variant, string> = {
    primary: "bg-accent text-on-accent hover:bg-accent-hover",
    ghost: "border border-border text-text hover:bg-surface-2 hover:border-border-strong",
    subtle: "text-muted hover:text-text hover:bg-surface-2",
    danger: "border border-border text-danger hover:bg-danger/10 hover:border-danger/40",
  };
  const branded = $derived(variant === "primary" && !!accent);
  const cls = $derived(`${base} ${sizes[size]} ${branded ? "" : variants[variant]} ${klass}`);
  const style = $derived(branded ? `background:${accent};color:${accentText(accent!)}` : undefined);
</script>

{#if href}
  <a {href} {target} {rel} class={cls} {style}>{@render children()}</a>
{:else}
  <button {type} {disabled} {onclick} class={cls} {style}>{@render children()}</button>
{/if}
