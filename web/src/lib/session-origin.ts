/**
 * What started a session, for the sidebar.
 *
 * Every momo-fetch process on the machine writes into one sessions table, so
 * the list has always held team workers and scheduled runs beside the
 * conversations — indistinguishable, a column of hex ids that all looked like
 * chats nobody remembered having. The gateway now stamps each one; this turns
 * that stamp into something readable.
 *
 * Mirrors `SessionOrigin` in `src/session.rs`. `null` means a chat somebody
 * typed, which is also what every session created before origins existed looks
 * like — the same answer, correctly.
 */

export type OriginKind = "chat" | "agent" | "worker" | "routine";

/**
 * The kind, or `"chat"` for anything unrecognised.
 *
 * An origin we cannot read must never be rendered as some *other* origin, so
 * unknown kinds fall back to the unmarked case rather than the nearest one.
 */
export function originKind(origin: string | null): OriginKind {
  // The colon is required, exactly as `SessionOrigin::parse` requires it: a
  // bare "routine" is not a routine, it is a string we do not understand.
  const at = origin?.indexOf(":") ?? -1;
  const kind = at === -1 ? undefined : origin?.slice(0, at);
  return kind === "agent" || kind === "worker" || kind === "routine"
    ? kind
    : "chat";
}

/** Everything after the first colon — an agent, worker or routine name. */
export function originName(origin: string): string {
  const at = origin.indexOf(":");
  return at === -1 ? "" : origin.slice(at + 1);
}

/**
 * The words, for everyone the glyph does not reach — a row tooltip, a screen
 * reader. `null` for a chat, which needs no explaining.
 */
export function originLabel(origin: string | null): string | null {
  if (!origin) return null;
  const name = originName(origin);
  switch (originKind(origin)) {
    case "agent":
      return `Started as agent ${name}`;
    case "worker":
      return `Team worker ${name}`;
    case "routine":
      return `Routine ${name}`;
    default:
      return null;
  }
}

/**
 * Glyph and colour per kind.
 *
 * The glyph carries the meaning, not the colour: the palette has three accents
 * and colour alone is the distinction a colour-blind reader loses (spec §6).
 */
export const ORIGIN_MARKS: Record<
  OriginKind,
  { glyph: string; color: string } | null
> = {
  chat: null,
  agent: { glyph: "◆", color: "text-signal" },
  worker: { glyph: "▣", color: "text-consent" },
  routine: { glyph: "◷", color: "text-dim" },
};
