<script lang="ts">
  let { data } = $props();

  // Step-1 skeleton dashboard. Destinations, upload links, and sync rules arrive in
  // later build steps.
  const cards = [
    { title: "Destinations", hint: "Frame.io folders to receive uploads" },
    { title: "Upload links", hint: "Branded links for external collaborators" },
    { title: "Sync rules", hint: "Auto-download new files to local storage" },
  ];
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-semibold">Dashboard</h1>
    <p class="text-sm text-neutral-500">Signed in as {data.user.email}</p>
  </div>

  <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
    {#each cards as card (card.title)}
      <div class="rounded-xl border border-neutral-200 bg-white p-5">
        <h2 class="font-medium">{card.title}</h2>
        <p class="mt-1 text-sm text-neutral-500">{card.hint}</p>
        <p class="mt-4 text-xs text-neutral-400">Coming in a later build step</p>
      </div>
    {/each}
  </div>
</div>
