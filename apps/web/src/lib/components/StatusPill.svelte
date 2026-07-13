<script lang="ts">
  let {
    status,
    label = undefined,
  }: { status: string; label?: string } = $props();

  // Semantic tone per status value, across links / uploads / sync jobs.
  const tones: Record<string, string> = {
    // good
    ok: "bg-success/15 text-success",
    completed: "bg-success/15 text-success",
    done: "bg-success/15 text-success",
    connected: "bg-success/15 text-success",
    // in flight
    in_progress: "bg-info/15 text-info",
    running: "bg-info/15 text-info",
    pending: "bg-info/15 text-info",
    waiting: "bg-warning/15 text-warning",
    // warn
    expired: "bg-warning/15 text-warning",
    incomplete: "bg-warning/15 text-warning",
    // bad
    revoked: "bg-surface-3 text-faint",
    skipped: "bg-surface-3 text-faint",
    failed: "bg-danger/15 text-danger",
    dead_letter: "bg-danger/15 text-danger",
  };
  const tone = $derived(tones[status] ?? "bg-surface-3 text-muted");
  const text = $derived((label ?? status).replace(/_/g, " "));
</script>

<span class="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium {tone}">
  {text}
</span>
