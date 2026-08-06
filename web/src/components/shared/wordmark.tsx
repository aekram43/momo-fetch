"use client";

/**
 * The MOMO WORK lockup: mark plus wordmark.
 *
 * Rebuilt from the brand assets rather than shipping the raster lockup, because
 * the wordmark has to recolour with the theme — the supplied lockups are baked
 * onto a navy or white ground and would sit in a visible box on the wrong one.
 *
 * **"MOMO" is not painted with the brand orange here**, even though the brand
 * lockup is. In this app orange means "the agent is working, or wants something
 * from you"; a permanently orange wordmark in the corner would be a standing
 * false alarm and would blunt the colour everywhere it actually carries
 * meaning. The mark keeps its orange bone — a few pixels, unmistakably the logo
 * — and the words take the ink colour, which *is* the brand navy in light mode.
 */
export function Wordmark() {
  return (
    <span className="flex items-center gap-2">
      {/* eslint-disable-next-line @next/next/no-img-element -- next/image does
          not prefix `basePath` for unoptimized static assets: it emitted
          `/momo-mark.png` while the bundle is mounted at `/ui`, so the image
          404'd and rendered as a broken-image box. A *relative* src sidesteps
          the whole question — it resolves to `/ui/…` under the gateway and to
          `/…` under Tauri, which are exactly the two places this runs. */}
      <img
        src="momo-mark-64.png"
        alt=""
        width={22}
        height={22}
        className="shrink-0"
      />
      <span className="text-[13px] font-semibold tracking-tight text-ink">
        MOMO<span className="font-normal tracking-[0.2em] text-dim"> WORK</span>
      </span>
    </span>
  );
}
