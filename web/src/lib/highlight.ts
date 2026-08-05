/**
 * F24 — Shiki highlighting.
 *
 * Deferred out of WP-4 for a reason that shapes this file: a highlighter must be
 * created **once** and reused. `createHighlighter` loads WASM plus grammar and
 * theme JSON, so calling it per code block — during a stream that re-renders the
 * same growing block on every token — would be pathological.
 *
 * So: one lazily-created singleton, a fixed language allow-list bundled at build
 * time (a static export cannot fetch grammars at runtime), and callers highlight
 * only when a block is *complete*.
 */

import type { Highlighter } from "shiki";

/** Grammars bundled up front. Anything else renders as plain text. */
const LANGS = [
  "rust", "typescript", "tsx", "javascript", "jsx", "python", "go",
  "bash", "shell", "json", "yaml", "toml", "sql", "html", "css", "markdown", "diff",
] as const;

const THEME = "vitesse-dark";

let instance: Promise<Highlighter> | null = null;

function highlighter(): Promise<Highlighter> {
  // The promise itself is the singleton, so concurrent callers during a stream
  // share one initialisation rather than racing several.
  instance ??= import("shiki").then((shiki) =>
    shiki.createHighlighter({ themes: [THEME], langs: [...LANGS] }),
  );
  return instance;
}

export function isSupportedLang(lang: string): boolean {
  return (LANGS as readonly string[]).includes(lang);
}

/**
 * Highlight `code`, returning HTML.
 *
 * Returns `null` when the language is unsupported or Shiki fails, and the caller
 * falls back to plain `<pre>`. Never throws — a highlighting failure must not
 * take out the message it was decorating.
 */
export async function highlight(
  code: string,
  lang: string,
): Promise<string | null> {
  if (!isSupportedLang(lang)) return null;
  try {
    const hl = await highlighter();
    return hl.codeToHtml(code, {
      lang,
      theme: THEME,
      // Shiki escapes the code it emits; this HTML is safe to inject.
      structure: "inline",
    });
  } catch {
    return null;
  }
}
