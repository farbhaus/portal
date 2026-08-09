import adapter from "@sveltejs/adapter-node";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter(),
    // We register the worker ourselves, from the download page only (see lib/zipdownload.ts): Kit's
    // auto-registration injects it into every page, and the admin app has no use for a worker.
    serviceWorker: { register: false },
  },
};

export default config;
