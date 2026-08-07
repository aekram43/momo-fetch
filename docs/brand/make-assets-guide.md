# make-assets.py — Function Reference

## Overview

`make-assets.py` derives app icons and web assets from `brand/source/brand-sheet.png` by cropping predefined regions, keying out the background, and resizing to final dimensions. It uses only Python stdlib.

Invocation: `python3 brand/make-assets.py`

## Entry point

```python
if __name__ == "__main__":
```

Reads the brand sheet, processes it into three assets, and outputs:
- `brand/app-icon.png` — 1024×1024
- `brand/mark.png` — 512×512
- `brand/lockup.png` — 1200×~470
- `web/public/momo-mark-64.png` — 64×64
- `web/public/favicon.png` — 64×64

## Image I/O

### `read_png(path)`

Reads a PNG file without external libraries. Parses the PNG chunk structure by hand, decompresses IDAT data, and reconstructs pixels. Normalizes all colour models to RGBA.

Returns `(width, height, pixels)` where pixels is a flat bytearray of RGBA bytes.

### `write_png(path, w, h, px)`

Writes a PNG file from raw RGBA pixels. Encodes each scanline with PNG filtering, compresses with zlib, and constructs the PNG chunk structure by hand.

## Pixel processing

### `clear_border_white(w, h, px, thresh=232)`

Flood-fills white from the edges, making the background transparent while preserving white inside the mark (like the dog's blaze and eye highlights).

Uses a threshold: any pixel with R≥`thresh`, G≥`thresh`, B≥`thresh` is considered "near white" and a candidate for removal. Only clears pixels connected to the border, so interior white is safe.

**Why this approach:** Keying by colour alone would punch holes straight through the face. Flood-filling from edges only clears the field.

Returns the modified pixel array.

### `defringe(w, h, px)`

Fades the pale halo left along a keyed edge.

`clear_border_white()` only clears pixels above the white threshold, so the anti-aliased ring between the white sheet background and the navy shape — mid-greys, below the threshold — survives as an opaque light outline. This reads as a deliberate stroke around the badge, which it is not.

For each opaque pixel touching a cleared one, drops alpha in proportion to how close it is to white (lightest ≥150), so the edge fades out.

Returns the modified pixel array.

### `circle_mask(w, h, px, feather=1.2)`

Clips the image to the inscribed circle with a soft edge.

For the round badge, geometric masking is used instead of colour-keying, because keying leaves the anti-aliased ring between the sheet background and the navy disc — a mid-grey too dark for the white threshold — which renders as a pale outline around the badge at small sizes.

The shape is a known circle, so masking it geometrically gives an exact edge instead of guessing from colour. The `feather` parameter softens the edge by that many pixels.

Returns the modified pixel array.

### `crop(w, h, px, x0, y0, x1, y1)`

Extracts a rectangular region from the image, inclusive of both corners.

Returns `(cropped_width, cropped_height, cropped_pixels)`.

## Resizing

### `upscale(w, h, px, tw, th)`

Resizes using bilinear interpolation. Used when enlarging.

Bilinear weights can round a 255 to 256, so output is clamped to [0, 255].

Returns the resized pixel array.

### `scale(w, h, px, tw, th)`

Resizes using nearest-neighbour with 3×3 supersampling. Used for downscaling flat vector-style art.

The supersampling is weighted by alpha (premultiplied), so semi-transparent edges blend correctly. Adequate for downscaling without adding a dependency.

Returns the resized pixel array.

## Composition

### `composite(fg_w, fg_h, fg, size, bg, margin=0.14)`

Centres the foreground image on a solid-colour background tile, leaving a margin so an OS icon mask does not clip the ears.

The foreground is scaled to fit inside `size × (1 - margin × 2)`, then centred. Compositing uses alpha blending.

Returns a `size × size` pixel array on the background colour.

## Constants

```python
NAVY = (6, 26, 43)       # #061a2b, sampled from the brand sheet
ORANGE = (247, 105, 21)  # #f76915
```

Crop coordinates into the source sheet:

```python
ICON_BOX = (763, 615, 970, 824)      # the "ICON" squircle
FAVICON_BOX = (1189, 633, 1373, 825)  # the "FAVICON" circle
LOCKUP_BOX = (60, 620, 600, 830)      # the "LOGO" horizontal lockup
```

## Workflow

**For the app icon (1024×1024):**
1. Crop ICON_BOX from sheet
2. Clear border white (thresh=232)
3. Defringe
4. Upscale 5× to 1024
5. Write to `brand/app-icon.png`

**For the circular badge (512×512):**
1. Crop FAVICON_BOX from sheet
2. Square it on its centre so the circle is concentric, not clipped
3. Apply circle mask
4. Upscale 5× to 512
5. Write to `brand/mark.png`
6. Upscale 5× to 64 and write to `web/public/momo-mark-64.png`
7. Upscale 5× to 64 and write to `web/public/favicon.png`

**For the horizontal lockup (1200×~470):**
1. Crop LOCKUP_BOX from sheet
2. Clear border white (thresh=240, slightly looser)
3. Upscale proportionally to 1200 wide
4. Write to `brand/lockup.png`

## Related reading

- [`code-guide.md`](code-guide.md) — design decisions and testing
- [`../../brand/README.md`](../../brand/README.md) — rationale for the pre-composed tiles approach
