#!/usr/bin/env python3
"""Render assets/icon-1024.png — the macclean app icon.

Stdlib only (zlib + struct): writes a 1024x1024 RGBA PNG. Shapes are
signed-distance functions with ~1.4 px analytic antialiasing. Motif: a
rounded-square (macOS icon shape) with an open ring — a disk mid-sweep.

Usage: scripts/make_icon.py            # writes assets/icon-1024.png
"""

import math
import struct
import zlib
from pathlib import Path

S = 1024
# Apple's icon grid: the rounded square spans ~824 px of the 1024 canvas,
# the rest is transparent margin. Full-bleed renders oversized in the Dock.
SHAPE = 824
OUT = Path(__file__).resolve().parent.parent / "assets" / "icon-1024.png"

# --- palette ----------------------------------------------------------------
BG_TOP = (26, 32, 44)      # #1a202c dark slate
BG_BOT = (13, 17, 23)      # #0d1117
RING_A = (60, 201, 126)    # #3cc97e — the app's "safe" green
RING_B = (56, 189, 248)    # #38bdf8 — teal, bottom half of the ring
DOT = (230, 162, 60)       # #e6a23c — the app's "review" amber


def smoothstep(edge0, edge1, x):
    t = min(max((x - edge0) / (edge1 - edge0), 0.0), 1.0)
    return t * t * (3 - 2 * t)


def sdf_rrect(px, py, cx, cy, hw, hh, r):
    """Signed distance to a rounded rect centered at (cx, cy)."""
    qx, qy = abs(px - cx) - (hw - r), abs(py - cy) - (hh - r)
    return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - r


def sdf_arc(px, py, cx, cy, radius, half_th, a0, a1):
    """Signed distance to an arc band with round caps.
    a0..a1 are degrees CCW from +x axis (screen y is down, so CCW reads
    clockwise on screen)."""
    dx, dy = px - cx, py - cy
    rad = math.hypot(dx, dy)
    theta = math.degrees(math.atan2(dy, dx)) % 360.0
    mid, span = (a0 + a1) / 2, (a1 - a0) / 2
    da = (theta - mid + 540) % 360 - 180
    if abs(da) <= span:
        return abs(rad - radius) - half_th
    # outside the sector → distance to the nearer round cap
    end = a0 if da < 0 else a1
    ex = cx + radius * math.cos(math.radians(end))
    ey = cy + radius * math.sin(math.radians(end))
    return math.hypot(px - ex, py - ey) - half_th


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def render():
    img = bytearray(S * S * 4)
    AA = 1.4
    cx = cy = S / 2
    hw = hh = SHAPE / 2
    corner = SHAPE * 0.222  # close to Apple's continuous-corner shape

    R, TH = SHAPE * 0.30, SHAPE * 0.075  # ring radius / half-thickness
    A0, A1 = 105.0, 350.0             # open ring: gap bottom-left-ish
    DOT_ANG = math.radians(A1)        # sweeper dot sits at the gap end
    dot_r = TH * 1.35
    dot_x = cx + R * math.cos(DOT_ANG)
    dot_y = cy + R * math.sin(DOT_ANG)

    for y in range(S):
        for x in range(S):
            i = (y * S + x) * 4

            # background rounded square, vertical gradient
            d = sdf_rrect(x, y, cx, cy, hw, hh, corner)
            a_bg = smoothstep(AA / 2, -AA / 2, d)
            if a_bg <= 0:
                continue
            bg = lerp(BG_TOP, BG_BOT, min(max((y - (S - SHAPE) / 2) / SHAPE, 0), 1))

            # ring, green -> teal along its vertical extent
            dr = sdf_arc(x, y, cx, cy, R, TH, A0, A1)
            a_ring = smoothstep(AA / 2, -AA / 2, dr)
            t_ring = min(max((math.hypot(x - cx, 0) and (y - (cy - R))) / (2 * R), 0), 1)
            ring = lerp(RING_A, RING_B, t_ring)

            # sweeper dot
            dd = math.hypot(x - dot_x, y - dot_y) - dot_r
            a_dot = smoothstep(AA / 2, -AA / 2, dd)

            # composite: bg, ring, dot (dot on top)
            r_, g_, b_ = bg
            r_ = int(r_ + (ring[0] - r_) * a_ring)
            g_ = int(g_ + (ring[1] - g_) * a_ring)
            b_ = int(b_ + (ring[2] - b_) * a_ring)
            r_ = int(r_ + (DOT[0] - r_) * a_dot)
            g_ = int(g_ + (DOT[1] - g_) * a_dot)
            b_ = int(b_ + (DOT[2] - b_) * a_dot)

            img[i:i + 4] = bytes((r_, g_, b_, int(a_bg * 255)))
    return img


def write_png(path, rgba):
    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data))
        )

    raw = b"".join(
        b"\x00" + bytes(rgba[y * S * 4:(y + 1) * S * 4]) for y in range(S)
    )
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", S, S, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    path.write_bytes(png)


if __name__ == "__main__":
    write_png(OUT, render())
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")
