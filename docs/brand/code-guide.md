# Code Guide — Before You Change make-assets.py

## Why stdlib only

`make-assets.py` intentionally uses only Python's standard library — no Pillow, no image library at all. This means the repo can rebuild its own icons without adding a dependency, and without requiring developers to have specialized image software installed. This constraint is load-bearing; if you add an import, you break the rebuild step for everyone.

## Why it uses pre-composed tiles

The script crops and processes tiles from `brand/source/brand-sheet.png`, rather than deriving everything from the hero mark. This is not a limitation — it is a deliberate workaround for a real problem.

The hero mark sits on a white field that reaches the dog's muzzle through the gap at the dog's chin. When you flood-fill the white background to make it transparent, that connected white disappears, and the face comes out wrong — this has been verified twice on two different versions of the art.

Instead, the designer maintains purpose-built ICON and FAVICON tiles, already composed on the brand navy. These do not need keying at all; only their transparent corners are cleared (and only the border white, never the interior). This is safe because the navy shape encloses every interior white pixel.

See `README.md` for the full explanation.

## The coordinate system

The script uses fixed crop coordinates into the source sheet. If you edit the sheet's layout, you must update these constants at the top of the `if __name__ == "__main__":` block:

```python
ICON_BOX = (763, 615, 970, 824)      # the "ICON" squircle
FAVICON_BOX = (1189, 633, 1373, 825)  # the "FAVICON" circle
LOCKUP_BOX = (60, 620, 600, 830)      # the "LOGO" horizontal lockup
```

Each tuple is `(x0, y0, x1, y1)` in pixel coordinates. These were found by scanning for bounding boxes, not measured by eye.

If you need to re-derive them after moving things on the sheet, look for the `bbox` helper comment in the script.

## The pipeline stages

The functions run in sequence for each asset:

1. **`crop()`** — Extract the tile from the sheet
2. **`clear_border_white()` and `defringe()`** — Remove the white background, then fade the anti-aliased edge so it does not show as a pale ring
3. **For circular badge:** `circle_mask()` — Clip to inscribed circle with soft edge (replaces colour-based keying for geometric precision)
4. **`upscale()` or `scale()`** — Resize to final dimensions
5. **`write_png()`** — Write the result

See [make-assets-guide.md](make-assets-guide.md) for what each function does.

## Testing changes

Before committing changes to the script:

```bash
python3 brand/make-assets.py
# Inspect the outputs visually:
# - brand/app-icon.png should have a clean circular mark, no pale ring
# - brand/mark.png should be perfectly circular
# - brand/lockup.png should look like clean text
# - web/public/* should match

# Then rebuild the Tauri icons:
cd desktop/src-tauri && cargo tauri icon ../../brand/app-icon.png
```

The generated files commit to the repo (they are not .gitignored). This is intentional — the icons need to exist for the build, and regenerating them should be rare enough to track.

## Known limitations

- Resolution is limited by the source tiles, which are about 200×200 pixels on the sheet. The 1024 app icon is a ~5× bilinear upscale, which is smooth but soft at the largest sizes. A ≥1024px master or SVG would be worth asking the designer for.

## Related reading

- [`make-assets-guide.md`](make-assets-guide.md) — function-by-function reference
- [`../../brand/README.md`](../../brand/README.md) — full explanation of the constraints
