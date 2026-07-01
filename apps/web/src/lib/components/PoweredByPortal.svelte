<script lang="ts">
  // Shared footer for the public/unauthenticated pages (upload, download, error, login).
  // Links "Powered by Portal" to the project on GitHub (#27). The privacy notice (#7) addresses
  // link recipients (clients/collaborators), so it's only linked where they land — set
  // `privacy={false}` on operator-facing pages like login.
  let { class: klass = "", privacy = true }: { class?: string; privacy?: boolean } = $props();
</script>

<p class="mt-6 text-center text-xs text-faint {klass}">
  <a
    href="https://github.com/farbhaus/portal/"
    target="_blank"
    rel="noopener noreferrer"
    class="hover:text-text hover:underline">Powered by Portal</a
  >
  {#if privacy}
    <span class="mx-1.5 text-border" aria-hidden="true">·</span>
    <a href="/privacy" class="hover:text-text hover:underline">Privacy</a>
  {/if}
</p>
