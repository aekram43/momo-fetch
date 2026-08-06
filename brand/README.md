# MOMO WORK — brand

## Palette

| | Hex | Where it comes from |
|---|---|---|
| **Navy** | `#061a2b` | sampled from the brand sheet |
| **Orange** | `#f76915` | sampled from the brand sheet |

Both were read out of `source/brand-sheet.png` rather than eyeballed.

### How the orange is used in the app

The UI reserves one colour to mean *"the agent is working, or wants something
from you"* — that is the whole point of the design system, and it is what makes
an approval prompt impossible to miss. The brand also has exactly one accent.

Running both would put two oranges on screen carrying different meanings. So
**the brand orange is the state colour**: `--signal` is `#f76915` in dark mode,
where it clears WCAG AA on the navy at 5.87:1 and can therefore carry meaning
rather than only decorate.

Two consequences worth knowing before changing anything here:

- **Light mode darkens it to `#bf4605`.** `#f76915` on white is 3.05:1 — it fails
  AA, and a signal colour that reads as decoration has stopped being a signal.
- **The wordmark in the app is *not* orange**, even though the brand lockup is.
  A permanently orange word in the corner is a standing false alarm, and it
  blunts the colour everywhere it actually matters. The mark keeps its orange
  bone; the words take the ink colour, which is the brand navy in light mode
  anyway.

Full-colour lockups belong where brand belongs — the app icon, the installer,
docs, the website — not in the running chrome.

## Files

```
source/
  brand-sheet.png           the master artwork — everything derives from this
  mark.png                  earlier standalone mark, on white
  mark-circle-dark.png      earlier circle badge
  mark-squircle-dark.png    earlier squircle
  lockup*.png               earlier lockups, various grounds

app-icon.png  1024×1024, generated — Tauri icon source (the sheet's ICON tile)
mark.png       512×512, generated — badge (the sheet's FAVICON tile)
lockup.png    1200×~470, generated — horizontal lockup, for docs
```

`web/public/momo-mark-64.png` and `web/public/favicon.png` are generated from the
same FAVICON tile.

## Regenerating

```bash
python3 brand/make-assets.py                       # → brand/{mark,app-icon}.png
cd desktop/src-tauri && cargo tauri icon ../../brand/app-icon.png
```

`make-assets.py` is stdlib-only so the repo can rebuild its own icons without
adding an image dependency.

Two things it has to work around, both worth knowing before editing it:

- **It uses the sheet's purpose-built ICON and FAVICON tiles, not the hero
  mark.** The hero sits on a white field that reaches the dog's muzzle through
  the gap at its chin, so flood-filling the background removes the muzzle with
  it and the face comes out wrong — verified, twice, on two different versions
  of the art. The ICON and FAVICON tiles are already composed on navy and need
  no keying; only their outer corners are cleared, which is safe because the
  navy shape encloses every interior white.
- **Resolution is limited by the source.** The tiles are about 200×200 on the
  sheet, so the 1024 icon is a ~5× bilinear upscale — smooth, but soft at the
  largest sizes. It is fine everywhere an icon is actually seen. A ≥1024px
  master or an SVG is the one asset still worth asking the designer for.
