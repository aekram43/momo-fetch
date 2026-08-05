"use client";

import { useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { highlight, isSupportedLang } from "@/lib/highlight";

/**
 * Markdown for assistant output (F7), with Shiki highlighting (F24).
 *
 * `streaming` is the important prop: while a turn is in flight the trailing code
 * block grows on every token, and highlighting it each time would re-run Shiki
 * against a slightly longer string dozens of times per second. Blocks stay plain
 * until the turn settles, then highlight once.
 */
export function MessageContent({
  content,
  streaming = false,
}: {
  content: string;
  streaming?: boolean;
}) {
  return (
    <div className="text-sm leading-relaxed text-ink">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          p: ({ children }) => <p className="mb-3 last:mb-0">{children}</p>,
          ul: ({ children }) => (
            <ul className="mb-3 list-disc space-y-1 pl-5 last:mb-0">{children}</ul>
          ),
          ol: ({ children }) => (
            <ol className="mb-3 list-decimal space-y-1 pl-5 last:mb-0">{children}</ol>
          ),
          h1: ({ children }) => (
            <h1 className="mb-2 mt-4 text-base font-semibold first:mt-0">{children}</h1>
          ),
          h2: ({ children }) => (
            <h2 className="mb-2 mt-4 text-sm font-semibold first:mt-0">{children}</h2>
          ),
          h3: ({ children }) => (
            <h3 className="mb-1.5 mt-3 text-sm font-medium first:mt-0">{children}</h3>
          ),
          a: ({ children, href }) => (
            <a
              href={href}
              target="_blank"
              rel="noreferrer noopener"
              className="text-consent underline underline-offset-2"
            >
              {children}
            </a>
          ),
          blockquote: ({ children }) => (
            <blockquote className="mb-3 border-l-2 border-rule pl-3 text-dim last:mb-0">
              {children}
            </blockquote>
          ),
          table: ({ children }) => (
            // Wide tables scroll inside their own box; the panel never scrolls
            // sideways.
            <div className="mb-3 overflow-x-auto last:mb-0">
              <table className="w-full border-collapse text-xs">{children}</table>
            </div>
          ),
          th: ({ children }) => (
            <th className="border border-rule bg-raised px-2 py-1 text-left font-medium">
              {children}
            </th>
          ),
          td: ({ children }) => (
            <td className="border border-rule px-2 py-1">{children}</td>
          ),
          code: ({ className, children }) => {
            const isBlock = /language-/.test(className ?? "");
            if (!isBlock) {
              return (
                <code className="rounded bg-raised px-1 py-0.5 font-mono text-[0.85em] text-ink">
                  {children}
                </code>
              );
            }
            const lang = /language-(\w+)/.exec(className ?? "")?.[1] ?? "";
            return (
              <CodeBlock lang={lang} streaming={streaming}>
                {String(children)}
              </CodeBlock>
            );
          },
          pre: ({ children }) => <>{children}</>,
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}

function CodeBlock({
  lang,
  children,
  streaming,
}: {
  lang: string;
  children: string;
  streaming: boolean;
}) {
  const [copied, setCopied] = useState(false);
  // Highlighted HTML is stored *with* the source it was produced from, so a
  // stale result from a previous render can be told apart without a second
  // state field — and every setState stays inside the async callback, which is
  // what React 19's set-state-in-effect rule wants.
  const [highlighted, setHighlighted] = useState<{
    code: string;
    html: string;
  } | null>(null);

  const code = children.replace(/\n$/, "");
  const html = !streaming && highlighted?.code === code ? highlighted.html : null;

  useEffect(() => {
    // Skip entirely while streaming — see the component doc.
    if (streaming || !isSupportedLang(lang)) return;
    let cancelled = false;
    void highlight(code, lang).then((result) => {
      if (!cancelled && result) setHighlighted({ code, html: result });
    });
    return () => {
      cancelled = true;
    };
  }, [code, lang, streaming]);

  async function copy() {
    await navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }

  return (
    <div className="group mb-3 overflow-hidden rounded border border-rule last:mb-0">
      <div className="flex items-center justify-between border-b border-rule bg-raised px-2.5 py-1">
        <span className="font-mono text-[10px] uppercase tracking-wider text-dim">
          {lang || "text"}
        </span>
        <button
          type="button"
          onClick={() => void copy()}
          className="font-mono text-[10px] text-dim transition-colors hover:text-ink"
        >
          {/* Label reports the outcome, not the instruction, once it happens. */}
          {copied ? "copied" : "copy"}
        </button>
      </div>
      <pre className="overflow-x-auto bg-void px-3 py-2">
        {html ? (
          // Shiki escapes what it emits, so this is safe to inject.
          <code
            className="font-mono text-xs leading-relaxed"
            dangerouslySetInnerHTML={{ __html: html }}
          />
        ) : (
          <code className="font-mono text-xs leading-relaxed text-ink">
            {code}
          </code>
        )}
      </pre>
    </div>
  );
}
