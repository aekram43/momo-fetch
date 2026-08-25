"use client";

import { useCallback, useEffect, useState } from "react";

import { useUiStore } from "@/stores/ui-store";

/**
 * Fetch a gateway resource once on mount, with a manual `reload`.
 *
 * Exists because every panel in WP-5 needs the same three things — data, an
 * error, and a way to refetch after a mutation — and because React 19's
 * `react-hooks/set-state-in-effect` rejects calling an extracted fetcher
 * directly from an effect. Getting that pattern right once here beats getting
 * it subtly wrong in nine components.
 */
export function useGatewayResource<T>(
  fetcher: () => Promise<T>,
  deps: unknown[] = [],
  /**
   * Refetch every N ms as well.
   *
   * For state this tab does not cause and is not told about — a team worker
   * finishing in a tmux pane, a routine firing on the gateway's timer. Leave it
   * off for anything that only changes because someone clicked something here;
   * `serverStateNonce` already covers that, and a poll on top of it is just
   * traffic.
   */
  intervalMs?: number,
) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);
  // Refetch when any component reports that gateway state changed, so a switch
  // made in one panel is reflected in all of them immediately.
  const serverStateNonce = useUiStore((s) => s.serverStateNonce);

  const reload = useCallback(() => setNonce((n) => n + 1), []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const result = await fetcher();
        if (!cancelled) {
          setData(result);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) setError((err as Error).message ?? "Request failed.");
      }
    };
    void load();
    if (!intervalMs) {
      return () => {
        cancelled = true;
      };
    }
    const timer = setInterval(() => void load(), intervalMs);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nonce, serverStateNonce, intervalMs, ...deps]);

  return { data, error, reload };
}
