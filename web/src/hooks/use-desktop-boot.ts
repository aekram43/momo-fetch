"use client";

import { useEffect, useState } from "react";

import {
  getGatewayStderr,
  getStartupError,
  isDesktop,
  resolveDesktopGatewayUrl,
} from "@/lib/desktop";

export type BootState =
  | { status: "ready" }
  | { status: "starting" }
  | { status: "failed"; error: string; stderr?: string };

/** How long to keep looking for a desktop shell before deciding this is a browser. */
const DETECT_WINDOW_MS = 1500;

/**
 * Desktop startup gate (T2/T3).
 *
 * In a browser this settles on `ready` quickly — the gateway URL comes from the
 * page's own origin and there is no child process to wait for. Under Tauri it
 * waits for the shell to report a URL, or for a startup failure.
 *
 * **`window.__TAURI__` is not guaranteed to exist at mount.** An earlier version
 * sampled `isDesktop()` once — in a `useState` initialiser and again at the top
 * of the effect — and bailed permanently when the global had not appeared yet.
 * The app then behaved as an ordinary browser tab and talked to
 * `localhost:3000`: visibly "offline" beside panels that all failed to load,
 * while other components checking `isDesktop()` on a *later* render correctly
 * showed desktop-only controls. Detection is therefore polled, not sampled.
 */
export function useDesktopBoot(): BootState {
  const [state, setState] = useState<BootState>({ status: "starting" });

  useEffect(() => {
    let cancelled = false;
    const startedAt = Date.now();

    const tick = async () => {
      if (cancelled) return;

      if (!isDesktop()) {
        // Give the shell a moment to install its global before concluding this
        // is an ordinary browser tab.
        if (Date.now() - startedAt > DETECT_WINDOW_MS) {
          setState({ status: "ready" });
        }
        return;
      }

      const url = await resolveDesktopGatewayUrl();
      if (cancelled) return;
      if (url) {
        setState({ status: "ready" });
        return;
      }

      const error = await getStartupError();
      if (cancelled || !error) return; // still starting
      const stderr = await getGatewayStderr();
      if (!cancelled) {
        setState({ status: "failed", error, stderr: stderr ?? undefined });
      }
    };

    void tick();
    const id = setInterval(() => void tick(), 250);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  return state;
}
