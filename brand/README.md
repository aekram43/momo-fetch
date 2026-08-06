# MOMO WORK — brand

## Palette

| | Hex | Where it comes from |
|---|---|---|
| **Navy** | `#041729` | sampled from the mark |
| **Orange** | `#f66614` | sampled from the bone |

Both were read out of `source/mark.png` rather than eyeballed.

### How the orange is used in the app

The UI reserves one colour to mean *"the agent is working, or wants something
from you"* — that is the whole point of the design system, and it is what makes
an approval prompt impossible to miss. The brand also has exactly one accent.

Running both would put two oranges on screen carrying different meanings. So
**the brand orange is the state colour**: `--signal` is `#f66614` in dark mode,
where it clears WCAG AA on the navy at 5.89:1 and can therefore carry meaning
rather than only decorate.

Two consequences worth knowing before changing anything here:

- **Light mode darkens it to `#bf4605`.** `#f66614` on white is 3.08:1 — it fails
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
source/     as supplied, untouched
  mark.png                  the mark alone, on white          366×414
  mark-circle-dark.png      mark in a navy circle             212×198
  mark-squircle-dark.png    mark in a navy squircle           232×218
  lockup.png                mark + wordmark, on white         526×198
  lockup-large.png          same, larger                     1083×414
  lockup-ink.png            navy wordmark                     519×178
  lockup-on-navy.png        reversed, on navy                 519×178
  lockup-on-orange.png      reversed, on orange               538×178

app-icon.png  1024×1024, generated — the Tauri icon source
mark.png       512×512, generated — transparent badge, used by the web header
```

## Regenerating

```bash
python3 brand/make-assets.py                       # → brand/{mark,app-icon}.png
cd desktop/src-tauri && cargo tauri icon ../../brand/app-icon.png
```

`make-assets.py` is stdlib-only so the repo can rebuild its own icons without
adding an image dependency.

Two things it has to work around, both worth knowing before editing it:

- **It derives from the *circle* asset, not the bare mark.** `mark.png` sits on a
  white field that reaches the dog's muzzle through the gap at its chin, so
  flood-filling the background from the border removes the muzzle with it and
  the face comes out wrong. The navy ring in the circle version encloses every
  interior white, so the fill stops where it should.
- **Resolution is limited by the source.** The circle asset is 212×198, so the
  1024×1024 icon is upscaled and soft at the largest sizes. It is fine at the
  sizes an icon is actually seen at. A ≥1024px original, or an SVG, would fix it
  properly — that is the one asset worth asking the designer for.
