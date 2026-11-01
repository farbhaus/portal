<script lang="ts">
  import { LinkComposer } from "$lib/components";
  import { lastFolder, recentFolders } from "$lib/recents";

  let { data } = $props();
  const recents = $derived(recentFolders(data.recentsFrom));
</script>

<LinkComposer
  mode="create"
  kind={data.kind}
  {recents}
  rules={data.rules}
  projectFolders={data.projectFolders}
  branding={data.branding}
  initialFolder={data.folder ?? lastFolder() ?? recents[0] ?? null}
  openSourcePicker={data.pick}
  emailConfigured={data.emailConfigured}
  presets={data.presets}
  brands={data.brands}
  defaultNotify={data.defaultNotify}
/>
