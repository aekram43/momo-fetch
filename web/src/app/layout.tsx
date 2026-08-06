import type { Metadata } from "next";
import { IBM_Plex_Sans, JetBrains_Mono } from "next/font/google";
import "./globals.css";

// IBM Plex Sans for the interface voice — engineering provenance, and not the
// geometric grotesque every dashboard reaches for.
const plex = IBM_Plex_Sans({
  variable: "--font-plex",
  subsets: ["latin"],
  weight: ["400", "500", "600"],
});

// JetBrains Mono for machine speech: tool names, paths, ids, counts.
const jetbrains = JetBrains_Mono({
  variable: "--font-jetbrains",
  subsets: ["latin"],
  weight: ["400", "500"],
});

export const metadata: Metadata = {
  title: "MoMo Worker",
  description: "Supervise a momo-fetch agent: chat, tools, memory, approvals.",
};

/**
 * Resolve and stamp the theme *before* first paint.
 *
 * React state cannot do this: the export is prerendered, so the served HTML has
 * no theme on it and the first frame is whatever the CSS defaults to. A user on
 * light would get a dark flash on every launch — the exact opposite of easier on
 * the eyes.
 *
 * Kept as a string rather than an imported function because it must run inline
 * in `<head>`, before the bundle loads. The key is duplicated from
 * `preferences.ts`; it is asserted in `preferences.test.ts` so the two cannot
 * drift silently.
 */
const themeBootScript = `
(function () {
  try {
    var raw = localStorage.getItem('momo-worker.prefs.v1');
    var choice = raw ? (JSON.parse(raw).theme || 'auto') : 'auto';
    var light = choice === 'light' ||
      (choice === 'auto' &&
        window.matchMedia('(prefers-color-scheme: light)').matches);
    document.documentElement.dataset.theme = light ? 'light' : 'dark';
  } catch (e) {
    document.documentElement.dataset.theme = 'dark';
  }
})();
`;

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html
      lang="en"
      className={`${plex.variable} ${jetbrains.variable} h-full antialiased`}
      suppressHydrationWarning
    >
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeBootScript }} />
      </head>
      {/* The shell owns its own scrolling regions; the page itself never scrolls. */}
      <body className="h-full overflow-hidden">{children}</body>
    </html>
  );
}
