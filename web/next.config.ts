import type { NextConfig } from "next";

/** The Tauri bundle, which is served from the root rather than from `/ui`. */
const desktopBuild = process.env.MOMO_BASE_PATH === "none" || process.env.MOMO_BASE_PATH === "";

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
  // `npm run build:desktop` asks for the Tauri one.
  //
  // **The desktop value is the word `none`, not an empty string.** npm runs
  // package scripts through `cmd` on Windows whatever shell the caller is in,
  // and `MOMO_BASE_PATH= next build` there is not an assignment at all — cmd
  // reads it as a command name and answers "'MOMO_BASE_PATH' is not recognized
  // as an internal or external command", which is how the Windows leg of the
  // release build died. Passing a real word makes the switch expressible as a
  // plain environment variable on every platform, which is what CI does.
  //
  // An empty string still means the same thing, so the existing
  // `MOMO_BASE_PATH= …` form keeps working in a POSIX shell.
  basePath: desktopBuild ? "" : (process.env.MOMO_BASE_PATH ?? "/ui"),
};

export default nextConfig;
