"use client";

import { useEffect, useState } from "react";

import {
  ApiError,
  getProviderModels,
  refreshProviderModels,
  switchModel,
} from "@/lib/api-client";
import {
  loadRecentModels,
  recordRecentModel,
  shortlistModels,
} from "@/lib/recent-models";
import { useUiStore } from "@/stores/ui-store";
import { useToastStore } from "@/stores/toast-store";
import type { ProviderModels } from "@/lib/types";

/**
 * Choose the model within the current provider.
 *
 * The catalogue is queried from the provider, so it is real rather than a list
 * baked into this app that goes stale — but that means it can also fail, and
 * failing must not trap someone on the wrong model. When `available` is false
 * the select is replaced by a text field: type the name, switch, done. The
 * provider is the authority on whether the name is valid either way.
 *
 * OpenRouter returns several hundred models, so the closed list is five rows —
 * the current model and the four this browser reached for most recently — and
 * the full catalogue appears as soon as a filter is typed. Showing all of them
 * up front is not a choice offered, it is a search problem handed over.
 */
export function ModelPicker({
  provider,
  current,
}: {
  provider: string;
  current: string | null;
}) {
  const [data, setData] = useState<ProviderModels | null>(null);
  const [filter, setFilter] = useState("");
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const [recent, setRecent] = useState<string[]>([]);
  const push = useToastStore((s) => s.push);
  const bump = useUiStore((s) => s.bumpServerState);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void (async () => {
      try {
        const d = await getProviderModels(provider);
        if (!cancelled) setData(d);
      } catch {
        if (!cancelled) {
          setData({ provider, models: [], available: false, cached: false, error: null });
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open, provider]);

  async function choose(model: string) {
    if (busy || !model.trim()) return;
    setBusy(true);
    try {
      await switchModel(model.trim());
      recordRecentModel(provider, model.trim());
      bump();
      setOpen(false);
      setFilter("");
      setTyped("");
    } catch (err) {
      push({
        tone: "error",
        message:
          err instanceof ApiError && err.isTurnConflict
            ? "A turn is running. Finish or interrupt it first."
            : `Could not switch to ${model}.`,
      });
    } finally {
      setBusy(false);
    }
  }

  async function refresh() {
    setData(null);
    await refreshProviderModels(provider).catch(() => null);
    const d = await getProviderModels(provider).catch(() => null);
    if (d) setData(d);
  }

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => {
          // Read history here rather than in an effect: opening is an event,
          // and React 19 rejects a synchronous setState from an effect body.
          setRecent(loadRecentModels(provider));
          setOpen(true);
        }}
        className="flex w-full items-center gap-1 rounded border border-rule bg-void py-1 pl-1.5 pr-1.5 text-left font-mono text-[11px] text-ink transition-colors hover:border-signal"
        title={current ?? undefined}
      >
        <span className="min-w-0 flex-1 truncate">
          {current ?? "choose a model"}
        </span>
        <span aria-hidden className="shrink-0 text-[10px] text-faint">
          ▾
        </span>
      </button>
    );
  }

  const query = filter.trim().toLowerCase();
  // Empty filter shows the shortlist; typing opens the whole catalogue.
  const matches = query
    ? (data?.models.filter((m) => m.toLowerCase().includes(query)) ?? [])
    : shortlistModels(data?.models ?? [], current, recent);
  const hidden = query ? 0 : (data?.models.length ?? 0) - matches.length;

  return (
    <div className="rounded border border-rule bg-raised p-1.5">
      {!data ? (
        <p className="text-[10px] text-faint">Loading models…</p>
      ) : data.available ? (
        <>
          <input
            autoFocus
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={`Filter ${data.models.length} models…`}
            aria-label="Filter models"
            className="mb-1 w-full rounded border border-rule bg-void px-1.5 py-1 font-mono text-[10px] text-ink outline-none placeholder:text-faint"
          />
          <ul className="max-h-44 overflow-y-auto">
            {matches.slice(0, 200).map((m) => (
              <li key={m}>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void choose(m)}
                  className={`w-full truncate rounded px-1.5 py-0.5 text-left font-mono text-[10px] transition-colors ${
                    m === current
                      ? "bg-signal/15 text-signal"
                      : "text-dim hover:bg-void hover:text-ink"
                  }`}
                  title={m}
                >
                  {m}
                </button>
              </li>
            ))}
            {matches.length === 0 && (
              <li className="px-1.5 py-1 text-[10px] text-faint">No match.</li>
            )}
          </ul>
          {query && matches.length > 200 && (
            <p className="mt-1 text-[10px] text-faint">
              Showing 200 of {matches.length}. Keep typing to narrow it.
            </p>
          )}
          {hidden > 0 && (
            <p className="mt-1 text-[10px] text-faint">
              {hidden} more. Type to search them.
            </p>
          )}
        </>
      ) : (
        <>
          {/* Catalogue unavailable — never a dead end. */}
          <p className="mb-1 text-[10px] leading-relaxed text-faint">
            {data.error ?? "Could not list models."} Type one instead.
          </p>
          <div className="flex gap-1">
            <input
              autoFocus
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && void choose(typed)}
              placeholder="model name"
              aria-label="Model name"
              className="min-w-0 flex-1 rounded border border-rule bg-void px-1.5 py-1 font-mono text-[10px] text-ink outline-none placeholder:text-faint"
            />
            <button
              type="button"
              disabled={busy || !typed.trim()}
              onClick={() => void choose(typed)}
              className="shrink-0 rounded bg-signal px-1.5 py-1 font-mono text-[10px] font-semibold text-void disabled:opacity-40"
            >
              set
            </button>
          </div>
        </>
      )}

      <div className="mt-1 flex justify-between">
        <button
          type="button"
          onClick={() => void refresh()}
          className="font-mono text-[10px] text-faint transition-colors hover:text-ink"
        >
          refresh{data?.cached ? " (cached)" : ""}
        </button>
        <button
          type="button"
          onClick={() => setOpen(false)}
          className="font-mono text-[10px] text-faint transition-colors hover:text-ink"
        >
          close
        </button>
      </div>
    </div>
  );
}
