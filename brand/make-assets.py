#!/usr/bin/env python3
"""Derive app icons and web assets from the brand source art.

The source marks are PNGs on a white background, not transparent, so pasting one
onto a dark tile leaves a white box around it. This keys out the near-white
field, keeps the mark's own white (the dog's blaze, its eyes) by only clearing
pixels connected to the border, and composites onto the brand navy.

Pure stdlib on purpose — no Pillow — so the repo can regenerate its own icons
without adding an image dependency.
"""
import struct, zlib, sys
from collections import deque

NAVY = (6, 26, 43)       # #061a2b, sampled from the brand sheet
ORANGE = (247, 105, 21)  # #f76915


def read_png(path):
    d = open(path, "rb").read()
    assert d[:8] == b"\x89PNG\r\n\x1a\n", f"{path} is not a PNG"
    pos, idat = 8, b""
    w = h = bd = ct = None
    while pos < len(d):
        ln = struct.unpack(">I", d[pos:pos + 4])[0]
        typ = d[pos + 4:pos + 8]
        data = d[pos + 8:pos + 8 + ln]
        pos += 12 + ln
        if typ == b"IHDR":
            w, h, bd, ct = struct.unpack(">IIBB", data[:10])
        elif typ == b"IDAT":
            idat += data
        elif typ == b"IEND":
            break
    raw = zlib.decompress(idat)
    ch = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ct]
    bpp = ch * (bd // 8) or 1
    stride = (w * ch * bd + 7) // 8
    out, prev, i = bytearray(), bytearray(stride), 0
    for _ in range(h):
        f = raw[i]; i += 1
        line = bytearray(raw[i:i + stride]); i += stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
            if f == 1: line[x] = (line[x] + a) & 255
            elif f == 2: line[x] = (line[x] + b) & 255
            elif f == 3: line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        out += line
        prev = line
    # normalise to RGBA
    px = bytearray(w * h * 4)
    for y in range(h):
        for x in range(w):
            o = (y * w + x) * ch
            if ct == 6:   r, g, b, a = out[o], out[o+1], out[o+2], out[o+3]
            elif ct == 2: r, g, b, a = out[o], out[o+1], out[o+2], 255
            else:         r = g = b = out[o]; a = 255
            q = (y * w + x) * 4
            px[q], px[q+1], px[q+2], px[q+3] = r, g, b, a
    return w, h, px


def write_png(path, w, h, px):
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        raw += px[y * w * 4:(y + 1) * w * 4]
    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    open(path, "wb").write(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def clear_border_white(w, h, px, thresh=232):
    """Make the white *field* transparent, leaving white inside the mark alone.

    Flood-fills from the edges rather than keying every white pixel, so the dog's
    blaze and eye highlights survive — keying by colour alone would punch holes
    straight through the face.
    """
    seen = bytearray(w * h)
    q = deque()
    def near_white(i):
        o = i * 4
        return px[o] >= thresh and px[o+1] >= thresh and px[o+2] >= thresh
    for x in range(w):
        for i in (x, (h - 1) * w + x):
            if not seen[i] and near_white(i): seen[i] = 1; q.append(i)
    for y in range(h):
        for i in (y * w, y * w + w - 1):
            if not seen[i] and near_white(i): seen[i] = 1; q.append(i)
    while q:
        i = q.popleft()
        px[i * 4 + 3] = 0
        x, y = i % w, i // w
        for nx, ny in ((x-1,y),(x+1,y),(x,y-1),(x,y+1)):
            if 0 <= nx < w and 0 <= ny < h:
                j = ny * w + nx
                if not seen[j] and near_white(j):
                    seen[j] = 1; q.append(j)
    return px


def circle_mask(w, h, px, feather=1.2):
    """Clip to the inscribed circle with a soft edge.

    Used instead of colour-keying for the round badge. Keying leaves the
    anti-aliased ring between the white sheet background and the navy disc — a
    mid-grey too dark for a white threshold to catch — which renders as a pale
    outline around the badge at small sizes. The shape is a known circle, so
    masking it geometrically gives an exact edge instead of guessing from colour.
    """
    out = bytearray(px)
    cx, cy = (w - 1) / 2, (h - 1) / 2
    r = min(w, h) / 2
    for y in range(h):
        for x in range(w):
            d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
            i = (y * w + x) * 4
            if d > r:
                out[i + 3] = 0
            elif d > r - feather:
                out[i + 3] = int(out[i + 3] * (r - d) / feather)
    return out


def defringe(w, h, px):
    """Fade the pale halo left along a keyed edge.

    `clear_border_white` only clears pixels *above* the white threshold, so the
    anti-aliased ring between the white sheet background and the navy shape —
    mid-greys, below the threshold — survives as an opaque light outline. At
    22px in the header that reads as a deliberate stroke around the badge, which
    it is not.

    For each opaque pixel touching a cleared one, drop alpha in proportion to how
    close it is to white, so the edge fades out instead of ending in a ring.
    """
    out = bytearray(px)
    for y in range(h):
        for x in range(w):
            i = y * w + x
            if px[i * 4 + 3] == 0:
                continue
            touches_cleared = False
            for nx, ny in ((x-1,y),(x+1,y),(x,y-1),(x,y+1)):
                if 0 <= nx < w and 0 <= ny < h and px[(ny*w+nx)*4+3] == 0:
                    touches_cleared = True
                    break
            if not touches_cleared:
                continue
            o = i * 4
            lightest = max(px[o], px[o+1], px[o+2])
            if lightest > 150:
                # 150 → keep, 255 → gone.
                out[o + 3] = int(px[o + 3] * (255 - lightest) / 105)
    return out


def crop(w, h, px, x0, y0, x1, y1):
    cw, ch = x1 - x0 + 1, y1 - y0 + 1
    out = bytearray(cw * ch * 4)
    for y in range(ch):
        src = ((y + y0) * w + x0) * 4
        out[y * cw * 4:(y + 1) * cw * 4] = px[src:src + cw * 4]
    return cw, ch, out


def upscale(w, h, px, tw, th):
    """Bilinear. Used when enlarging, where the supersampler in `scale` degrades
    to nearest-neighbour and leaves stair-stepped edges on the curves."""
    out = bytearray(tw * th * 4)
    for y in range(th):
        fy = (y + 0.5) * h / th - 0.5
        y0 = max(0, min(h - 1, int(fy))); y1 = min(h - 1, y0 + 1); wy = fy - y0
        for x in range(tw):
            fx = (x + 0.5) * w / tw - 0.5
            x0 = max(0, min(w - 1, int(fx))); x1 = min(w - 1, x0 + 1); wx = fx - x0
            q = (y * tw + x) * 4
            for c in range(4):
                a = px[(y0 * w + x0) * 4 + c] * (1 - wx) + px[(y0 * w + x1) * 4 + c] * wx
                b = px[(y1 * w + x0) * 4 + c] * (1 - wx) + px[(y1 * w + x1) * 4 + c] * wx
                # Clamp: bilinear weights can round a 255 to 256.
                out[q + c] = min(255, max(0, int(a * (1 - wy) + b * wy + 0.5)))
    return out


def scale(w, h, px, tw, th):
    """Nearest-neighbour with 3x3 supersampling — adequate for downscaling flat
    vector-style art, and keeps this dependency-free."""
    out = bytearray(tw * th * 4)
    for y in range(th):
        for x in range(tw):
            r = g = b = a = n = 0
            for sy in range(3):
                for sx in range(3):
                    ox = min(w - 1, int((x + (sx + 0.5) / 3) * w / tw))
                    oy = min(h - 1, int((y + (sy + 0.5) / 3) * h / th))
                    o = (oy * w + ox) * 4
                    al = px[o + 3]
                    r += px[o] * al; g += px[o+1] * al; b += px[o+2] * al
                    a += al; n += 1
            q = (y * tw + x) * 4
            if a:
                out[q], out[q+1], out[q+2] = r // a, g // a, b // a
            out[q + 3] = a // n
    return out


def composite(fg_w, fg_h, fg, size, bg, margin=0.14):
    """Centre `fg` on a `size`×`size` tile of `bg`, leaving a margin so an OS
    icon mask does not clip the ears."""
    inner = int(size * (1 - margin * 2))
    s = inner / max(fg_w, fg_h)
    sw, sh = max(1, int(fg_w * s)), max(1, int(fg_h * s))
    small = scale(fg_w, fg_h, fg, sw, sh)
    out = bytearray()
    for _ in range(size * size):
        out += bytes((bg[0], bg[1], bg[2], 255))
    ox, oy = (size - sw) // 2, (size - sh) // 2
    for y in range(sh):
        for x in range(sw):
            o = (y * sw + x) * 4
            al = small[o + 3]
            if not al: continue
            q = ((oy + y) * size + ox + x) * 4
            for c in range(3):
                out[q + c] = (small[o + c] * al + out[q + c] * (255 - al)) // 255
    return out


# Regions on the brand sheet, found by scanning for their bounding boxes rather
# than measured by eye. Re-derive with the bbox helper if the sheet is replaced.
SHEET = "brand/source/brand-sheet.png"
ICON_BOX = (763, 615, 970, 824)      # the "ICON" squircle
FAVICON_BOX = (1189, 633, 1373, 825)  # the "FAVICON" circle
LOCKUP_BOX = (60, 620, 600, 830)      # the "LOGO" horizontal lockup


if __name__ == "__main__":
    w, h, px = read_png(SHEET)

    # The sheet ships purpose-built ICON and FAVICON tiles, already composed on
    # navy by the designer. Use them rather than re-deriving from the hero mark:
    # the hero sits on a white field that reaches the dog's muzzle through the
    # gap at its chin, so flood-filling the background removes the muzzle with it
    # and the face comes out wrong. These need no keying at all.
    cw, ch, icon = crop(w, h, px, *ICON_BOX)
    # Clear the sheet background outside the squircle — an app icon with opaque
    # white corners shows them as a square halo behind every OS rounding.
    icon = defringe(cw, ch, clear_border_white(cw, ch, icon, thresh=232))
    write_png("brand/app-icon.png", 1024, 1024, upscale(cw, ch, icon, 1024, 1024))

    # Square the FAVICON box on its own centre before masking, so the circle is
    # concentric with the crop rather than clipped on the long axis.
    fx0, fy0, fx1, fy1 = FAVICON_BOX
    side = max(fx1 - fx0, fy1 - fy0)
    ccx, ccy = (fx0 + fx1) // 2, (fy0 + fy1) // 2
    fw, fh, fav = crop(w, h, px, ccx - side // 2, ccy - side // 2,
                       ccx + side // 2, ccy + side // 2)
    fav = circle_mask(fw, fh, fav)
    write_png("brand/mark.png", 512, 512, upscale(fw, fh, fav, 512, 512))
    write_png("web/public/momo-mark-64.png", 64, 64, upscale(fw, fh, fav, 64, 64))
    write_png("web/public/favicon.png", 64, 64, upscale(fw, fh, fav, 64, 64))

    # Horizontal lockup, for docs and the README.
    lw, lh, lock = crop(w, h, px, *LOCKUP_BOX)
    lock = clear_border_white(lw, lh, lock, thresh=240)
    tw = 1200
    write_png("brand/lockup.png", tw, int(tw * lh / lw),
              upscale(lw, lh, lock, tw, int(tw * lh / lw)))

    print("wrote brand/{app-icon,mark,lockup}.png and web/public/{momo-mark-64,favicon}.png")
