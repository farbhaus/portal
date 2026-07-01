<script lang="ts">
  // Branded fallback for thrown errors — most importantly the 404 that the public upload/download
  // pages raise for a missing, deleted, or mistyped link (routes/d, routes/u +page.server.ts).
  // Without this, SvelteKit renders its unstyled default page.
  import { page } from "$app/state";
  import { Card, PoweredByPortal } from "$lib/components";

  const status = $derived(page.status);
  const message = $derived(page.error?.message);
  const heading = $derived(status === 404 ? "Link not found" : "Something went wrong");
  const fallback = $derived(
    status === 404
      ? "This link doesn't exist, or it may have been removed. Check the address, or ask whoever shared it for a new one."
      : "An unexpected error occurred. Please try again in a moment.",
  );
</script>

<svelte:head><title>{status} · Portal</title></svelte:head>

<div class="flex min-h-screen flex-col items-center justify-center bg-bg px-4 py-12">
  <div class="w-full max-w-md">
    <div class="mb-6 text-center">
      <h1 class="text-2xl font-semibold tracking-tight">{heading}</h1>
    </div>
    <Card class="text-center">
      <p class="text-5xl font-semibold text-faint">{status}</p>
      <p class="mt-3 text-muted">{message || fallback}</p>
    </Card>
    <PoweredByPortal />
  </div>
</div>
