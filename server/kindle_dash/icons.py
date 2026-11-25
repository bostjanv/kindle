"""Weather icons drawn with primitives so they stay crisp on e-ink at any size."""
from __future__ import annotations

import math

from .render import BLACK, DARK, LIGHT, WHITE


def _sun(d, cx, cy, r, w):
    d.ellipse((cx - r, cy - r, cx + r, cy + r), outline=BLACK, width=w)
    for k in range(8):
        a = k * math.pi / 4
        d.line((cx + math.cos(a) * r * 1.4, cy + math.sin(a) * r * 1.4,
                cx + math.cos(a) * r * 1.85, cy + math.sin(a) * r * 1.85), fill=BLACK, width=w)


def _moon(d, cx, cy, r, w):
    d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=BLACK)
    o = r * 0.55
    d.ellipse((cx - r + o, cy - r - o * 0.4, cx + r + o, cy + r - o * 0.4), fill=WHITE)


def _cloud(d, x, y, s, fill=WHITE, w=3):
    """Cloud whose bounding box is (x, y, x+s, y+0.6s)."""
    h = s * 0.6
    parts = [
        (x + s * 0.05, y + h * 0.40, x + s * 0.45, y + h),      # left puff
        (x + s * 0.22, y + h * 0.05, x + s * 0.70, y + h * 0.85),  # top puff
        (x + s * 0.50, y + h * 0.30, x + s * 0.95, y + h),      # right puff
    ]
    for p in parts:  # outlines first, then fills, gives a single merged silhouette
        d.ellipse((p[0] - w, p[1] - w, p[2] + w, p[3] + w), fill=BLACK)
    d.rectangle((x + s * 0.25 - w, y + h * 0.6, x + s * 0.75 + w, y + h + w), fill=BLACK)
    for p in parts:
        d.ellipse(p, fill=fill)
    d.rectangle((x + s * 0.25, y + h * 0.6, x + s * 0.75, y + h - 0.5), fill=fill)


def draw_icon(d, code: int, x: int, y: int, size: int, night: bool = False):
    s = size
    w = max(2, s // 28)
    cx, cy = x + s / 2, y + s / 2
    if code in (0, 1):
        (_moon if night else _sun)(d, cx, cy, s * 0.24, w)
        return
    if code == 2:
        (_moon if night else _sun)(d, x + s * 0.62, y + s * 0.32, s * 0.16, w)
        _cloud(d, x + s * 0.02, y + s * 0.35, s * 0.8, w=w)
        return
    if code == 3:
        _cloud(d, x + s * 0.25, y + s * 0.15, s * 0.7, fill=LIGHT, w=w)
        _cloud(d, x, y + s * 0.3, s * 0.85, w=w)
        return
    if code in (45, 48):
        for k in range(5):
            yy = y + s * (0.25 + k * 0.13)
            off = s * 0.08 * (k % 2)
            d.line((x + s * 0.1 + off, yy, x + s * 0.9 - off, yy), fill=DARK, width=w + 1)
        return

    # precipitation / storms: cloud on top, symbols below
    _cloud(d, x + s * 0.05, y + s * 0.05, s * 0.9, fill=LIGHT if code >= 95 else WHITE, w=w)
    by = y + s * 0.65
    if code >= 95:
        bolt = [(cx + s * 0.05, by - s * 0.05), (cx - s * 0.12, by + s * 0.17), (cx, by + s * 0.17),
                (cx - s * 0.08, by + s * 0.35), (cx + s * 0.15, by + s * 0.1), (cx + s * 0.03, by + s * 0.1)]
        d.polygon(bolt, fill=BLACK)
    elif code in (71, 73, 75, 77, 85, 86):
        for k in range(3):
            fx, fy, r = x + s * (0.28 + k * 0.22), by + s * (0.1 + 0.1 * (k % 2)), s * 0.09
            for a in range(3):
                ang = a * math.pi / 3
                d.line((fx - math.cos(ang) * r, fy - math.sin(ang) * r, fx + math.cos(ang) * r, fy + math.sin(ang) * r),
                       fill=BLACK, width=w)
    else:  # drizzle / rain / showers
        drops = 2 if code in (51, 53, 55, 56, 57) else 3
        for k in range(drops):
            fx = x + s * (0.3 + k * (0.4 / max(1, drops - 1)))
            d.line((fx, by + s * 0.02, fx - s * 0.07, by + s * 0.25), fill=BLACK, width=w + 1)
