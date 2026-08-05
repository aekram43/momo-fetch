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

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html
      lang="en"
      className={`${plex.variable} ${jetbrains.variable} h-full antialiased`}
    >
      {/* The shell owns its own scrolling regions; the page itself never scrolls. */}
      <body className="h-full overflow-hidden">{children}</body>
    </html>
  );
}
