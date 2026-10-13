import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    // Client-rendered SPA: every route is emitted as static HTML plus a fallback shell
    // (index.html) that a static file server (the Rust server) serves for any route.
    adapter: adapter({ fallback: "index.html" }),
    // We register the worker ourselves, from the download page only (see lib/zipdownload.ts): Kit's
    // auto-registration injects it into every page, and the admin app has no use for a worker.
    serviceWorker: { register: false },
  },
};

export default config;
