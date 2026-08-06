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

NAVY = (4, 23, 41)      # #041729
ORANGE = (246, 102, 20)  # #f66614


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


if __name__ == "__main__":
    # The circle lockup, not the bare mark.
    #
    # `mark.png` sits on a white field that reaches the dog's muzzle through the
    # gap at its chin, so flood-filling from the border removes the muzzle along
    # with the background and the face comes out wrong. In the circle version the
    # navy ring encloses every interior white, so the fill stops where it should
    # and only the four corners clear.
    w, h, px = read_png("brand/source/mark-circle-dark.png")
    px = clear_border_white(w, h, px, thresh=int(sys.argv[1]) if len(sys.argv) > 1 else 232)

    # Badge with transparent corners — legible on light or dark, so one asset
    # serves both themes.
    size = 512
    write_png("brand/mark.png", size, size, scale(w, h, px, size, size))

    # Square app icon: the badge on brand navy, with margin so an OS mask does
    # not clip the ears.
    write_png("brand/app-icon.png", 1024, 1024,
              composite(w, h, px, 1024, NAVY, margin=0.06))
    print("wrote brand/mark.png and brand/app-icon.png")
