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

  // Next emits **absolute** asset URLs, so the bundle has to know where it will
  // be mounted — and the two consumers mount it in different places:
  //
  //   gateway (G9)  serves it at /ui  → needs basePath "/ui"
  //   Tauri (T1)    serves it at /    → needs basePath ""
  //
  // Get this wrong in either direction and every stylesheet and script 404s,
  // rendering an unstyled page. It is easy to mis-diagnose as a CSS problem:
  // fetching the same assets by hand *with* the right prefix returns 200.
  //
  // So it is a build-time switch. `npm run build` keeps the gateway default;
  // `npm run build:desktop` sets MOMO_BASE_PATH="" for the Tauri bundle.
  basePath: process.env.MOMO_BASE_PATH ?? "/ui",
};

export default nextConfig;
