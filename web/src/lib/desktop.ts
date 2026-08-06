/**
 * Desktop-shell integration.
 *
 * The web bundle is identical in the browser and inside Tauri, so every entry
 * point here is a no-op when the shell is absent. Nothing in the UI may *depend*
 * on running under Tauri.
 *
 * Tauri is reached through `window.__TAURI__` rather than the `@tauri-apps/api`
 * package: the package would be bundled into the browser build for a feature
 * only the desktop uses, and the global is present exactly when the shell is.
 */

interface TauriGlobal {
  core?: { invoke?: (cmd: string, args?: unknown) => Promise<unknown> };
}

declare global {
  interface Window {
    __TAURI__?: TauriGlobal;
  }
}

export function isDesktop(): boolean {
  return typeof window !== "undefined" && Boolean(window.__TAURI__?.core?.invoke);
}

async function invoke<T>(cmd: string, args?: unknown): Promise<T | null> {
  const fn = window.__TAURI__?.core?.invoke;
  if (!fn) return null;
  try {
    return (await fn(cmd, args)) as T;
  } catch {
    return null;
  }
}

/**
 * Ask the shell for the gateway URL and cache it on `window`.
 *
 * The shell also injects `window.__GATEWAY_URL__` via `eval` at setup time, but
 * that races the page's own scripts — `eval` runs whenever the webview is ready,
 * which is not guaranteed to be before the bundle boots. This command is the
 * authoritative read; the injection just makes the common case instant.
 */
export async function resolveDesktopGatewayUrl(): Promise<string | null> {
  if (!isDesktop()) return null;
  if (window.__GATEWAY_URL__) return window.__GATEWAY_URL__;
  const url = await invoke<string | null>("gateway_url");
  if (url) window.__GATEWAY_URL__ = url;
  return url;
}

/** Per-provider key status. Note there is no "get key" — by design. */
export interface SecretStatus {
  provider: string;
  env_var: string;
  configured: boolean;
  also_in_env_file: boolean;
}

export const getSecretStatus = () => invoke<SecretStatus[]>("secret_status");

/**
 * Store an API key in the OS keychain and restart the gateway.
 *
 * Desktop only. The key goes over Tauri IPC to the shell process — never over
 * HTTP, and never to the gateway, which receives it as an environment variable
 * when the shell respawns it.
 *
 * There is deliberately no way to read a key back out.
 */
export async function setSecret(
  provider: string,
  key: string,
): Promise<string | null> {
  const fn = window.__TAURI__?.core?.invoke;
  if (!fn) return "Not running in the desktop app.";
  try {
    await fn("set_secret", { provider, key });
    return null;
  } catch (e) {
    return String(e);
  }
}

export async function deleteSecret(provider: string): Promise<string | null> {
  const fn = window.__TAURI__?.core?.invoke;
  if (!fn) return "Not running in the desktop app.";
  try {
    await fn("delete_secret", { provider });
    return null;
  } catch (e) {
    return String(e);
  }
}

/** Startup failure from the shell — the gateway never came up. */
export const getStartupError = () => invoke<string | null>("startup_error");

/** Last lines of gateway stderr, for the crash screen. */
export const getGatewayStderr = () => invoke<string>("gateway_stderr");

/**
 * **T4** — re-root the gateway at another project.
 *
 * The caller must have confirmed first. Re-rooting moves the sandbox, the memory
 * vault and the session DB, and a deep link (T9) can propose one, so the
 * confirmation is a security boundary rather than politeness.
 */
export const openProject = (path: string) =>
  invoke<string>("open_project", { path });

/**
 * Menu and tray actions (T6/T7) arrive as one `momo:menu` CustomEvent carrying
 * the item id, so a given action has a single implementation regardless of
 * whether it came from a menu, the tray, or a keystroke.
 */
export type MenuAction =
  | "new-session"
  | "open-project"
  | "toggle-sidebar"
  | "toggle-detail"
  | "settings"
  | "interrupt";

/**
 * **T9** — a `momo://open?path=…` link proposing a project directory.
 *
 * The shell has already validated the path (absolute, no traversal, exists, is
 * a directory) — but validation is not authorisation. A deep link can be fired
 * by any web page, and opening a project re-roots the sandbox, so this only
 * ever *proposes*. The handler must show the same confirmation the Files panel
 * uses before calling {@link openProject}.
 */
export function onDeepLinkOpenRequest(
  handler: (path: string) => void,
): () => void {
  if (typeof window === "undefined") return () => {};
  const listener = (e: Event) => {
    const detail = (e as CustomEvent<{ path?: string }>).detail;
    if (detail?.path) handler(detail.path);
  };
  window.addEventListener("momo:open-project-request", listener);
  return () =>
    window.removeEventListener("momo:open-project-request", listener);
}

/** A link the shell refused, with the reason, so the UI can say why. */
export function onDeepLinkRejected(
  handler: (reason: string) => void,
): () => void {
  if (typeof window === "undefined") return () => {};
  const listener = (e: Event) => {
    const detail = (e as CustomEvent<string>).detail;
    if (detail) handler(detail);
  };
  window.addEventListener("momo:deep-link-rejected", listener);
  return () => window.removeEventListener("momo:deep-link-rejected", listener);
}

export function onMenuAction(handler: (action: MenuAction) => void): () => void {
  if (typeof window === "undefined") return () => {};
  const listener = (e: Event) => {
    const detail = (e as CustomEvent<string>).detail;
    if (detail) handler(detail as MenuAction);
  };
  window.addEventListener("momo:menu", listener);
  return () => window.removeEventListener("momo:menu", listener);
}
