/**
 * The handful of models offered before anyone types.
 *
 * A provider catalogue is hundreds of entries, and an alphabetical wall of them
 * is not a choice — it is a search problem handed to the user. But almost
 * nobody uses hundreds: they move between two or three and stay there. So the
 * closed list is short and made of the models *this browser has actually
 * switched to*, with the current one pinned first, and the full catalogue
 * appears the moment a filter is typed.
 *
 * This is local view state, not server state: it records where this browser has
 * been, never what the harness is running now. The current model always comes
 * from `/health` (spec F28) — a remembered "current" would be a belief that goes
 * wrong as soon as another tab or the REPL switches.
 */

const KEY = "momo-worker.recent-models.v1";

/** Kept per provider. Deeper than the shortlist so history survives a switch. */
const CAP = 8;

/** Rows shown with an empty filter: the current model plus four. */
export const SHORTLIST_SIZE = 5;

type Store = Record<string, string[]>;

function read(): Store {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as Store) : {};
  } catch {
    return {};
  }
}

/** Most recent first. */
export function loadRecentModels(provider: string): string[] {
  const entry = read()[provider];
  return Array.isArray(entry) ? entry.filter((m) => typeof m === "string") : [];
}

export function recordRecentModel(provider: string, model: string): void {
  if (typeof window === "undefined") return;
  try {
    const store = read();
    const next = [model, ...(store[provider] ?? []).filter((m) => m !== model)];
    store[provider] = next.slice(0, CAP);
    window.localStorage.setItem(KEY, JSON.stringify(store));
  } catch {
    // Private browsing, quota, storage off. A shortlist is a convenience —
    // never fail a render over it.
  }
}

/**
 * The closed list: current first, then recents, then the head of the catalogue.
 *
 * Padding from the catalogue matters on a fresh install, where there is no
 * history at all and a one-row list would look broken. Everything offered is
 * something the provider listed, except the current model, which is included
 * even if the catalogue somehow omits it — the list must always be able to show
 * what is running.
 */
export function shortlistModels(
  models: string[],
  current: string | null,
  recent: string[],
  limit: number = SHORTLIST_SIZE,
): string[] {
  const out: string[] = [];
  const add = (m: string) => {
    if (out.length < limit && !out.includes(m)) out.push(m);
  };

  if (current) add(current);
  for (const m of recent) if (models.includes(m)) add(m);
  for (const m of models) add(m);
  return out;
}
