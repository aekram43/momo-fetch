import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Static export → `out/`. Tauri (Phase 2) loads these files off disk with no
  // Node server, and G9 serves the same directory at /ui/*.
  //
  // The cost, called out in spec §3.2: Route Handlers, Middleware, ISR and image
  // optimisation are all unavailable. Everything dynamic goes to the gateway
  // over HTTP, which is why CORS on the gateway is load-bearing rather than a
  // nicety — there is no server-side proxy to hide behind.
  output: "export",

  // No server means no image optimiser.
  images: { unoptimized: true },

  // Emit `out/foo/index.html` rather than `out/foo.html`, so a plain static file
  // server resolves `/foo` and `/foo/` identically. G9's ServeDir + SPA fallback
  // rely on this.
  trailingSlash: true,

  // G9 mounts this bundle at /ui, and Next emits **absolute** asset URLs. Without
  // a basePath the built HTML links `/_next/static/…`, which is not under /ui —
  // the browser 404s every stylesheet and script and renders an unstyled page.
  // (Fetching those same assets *with* a /ui prefix by hand succeeds, which makes
  // this easy to mis-diagnose as a CSS problem.)
  //
  // Must match the gateway's mount point. It is inlined into the client bundle at
  // build time, so changing one without the other silently breaks asset loading.
  basePath: "/ui",
};

export default nextConfig;
