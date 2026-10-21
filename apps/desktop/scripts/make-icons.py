#!/usr/bin/env python3
"""Draws the Folyvo app icon and the menu bar glyph: four layers, thickness growing downwards.

Standard library only. Run from the repository root:
    python3 apps/desktop/scripts/make-icons.py
"""
import math
import os
import struct
import zlib

ICONS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src-tauri", "icons")
INK = (0x2C, 0x32, 0x37)
LAYERS = [(0xD4, 0xA3, 0x5A), (0x1F, 0x6F, 0x5C), (0x4F, 0x62, 0x5A), (0x5E, 0x70, 0x68)]
RATIOS = [1.0, 1.4, 1.9, 2.6]


def write_png(path, width, height, rows):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    raw = b"".join(b"\x00" + bytes(row) for row in rows)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    with open(path, "wb") as f:
        f.write(png)


def coverage(px, py, cx, cy, half, radius):
    """Anti-aliased coverage (0..1) of a rounded square, from its signed distance."""
    qx = abs(px - cx) - (half - radius)
    qy = abs(py - cy) - (half - radius)
    d = math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - radius
    return min(1.0, max(0.0, 0.5 - d))


def app_icon(n=1024):
    margin = n * 0.08
    half = (n - 2 * margin) / 2
    radius = half * 0.45
    cx = cy = n / 2
    inner_left, inner_right = cx - half * 0.68, cx + half * 0.68
    gap = n * 0.02
    usable = half * 1.3 - gap * (len(RATIOS) - 1)
    unit = usable / sum(RATIOS)
    y = cy - half * 0.65
    spans = []
    for ratio in RATIOS:
        spans.append((y, y + unit * ratio))
        y += unit * ratio + gap
    rows = []
    for py in range(n):
        row = bytearray()
        for px in range(n):
            a = coverage(px + 0.5, py + 0.5, cx, cy, half, radius)
            colour = INK
            if inner_left <= px <= inner_right:
                for (top, bottom), layer in zip(spans, LAYERS):
                    if top <= py <= bottom:
                        colour = layer
            row += bytes((*colour, round(a * 255)))
        rows.append(row)
    write_png(os.path.join(ICONS, "app-icon.png"), n, n, rows)


def tray_glyph(n=44):
    heights = [3, 4, 6, 8]
    bars, y = [], 7
    for h in heights:
        bars.append((y, y + h))
        y += h + 3
    rows = []
    for py in range(n):
        inside = any(top <= py < bottom for top, bottom in bars)
        row = bytearray()
        for px in range(n):
            on = inside and 7 <= px < n - 7
            row += bytes((0, 0, 0, 255 if on else 0))
        rows.append(row)
    write_png(os.path.join(ICONS, "tray.png"), n, n, rows)


if __name__ == "__main__":
    os.makedirs(ICONS, exist_ok=True)
    app_icon()
    tray_glyph()
    print("wrote app-icon.png and tray.png in", os.path.normpath(ICONS))
