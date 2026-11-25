"""Canvas, fonts and layout: turns the configured widget stack into a Kindle-ready PNG."""
from __future__ import annotations

import io
import logging
import time
from dataclasses import dataclass, field
from datetime import datetime
from functools import lru_cache
from pathlib import Path
from zoneinfo import ZoneInfo

from PIL import Image, ImageDraw, ImageFont



log = logging.getLogger(__name__)
FONT_DIR = Path(__file__).parent / "fonts"

BLACK, DARK, MID, LIGHT, WHITE = 0, 64, 128, 200, 255


@lru_cache(maxsize=64)
def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(str(FONT_DIR / ("DejaVuSans-Bold.ttf" if bold else "DejaVuSans.ttf")), size)


def text_width(draw: ImageDraw.ImageDraw, text: str, f) -> int:
    return int(draw.textlength(text, font=f))


def ellipsize(draw: ImageDraw.ImageDraw, text: str, f, max_w: int) -> str:
    if text_width(draw, text, f) <= max_w:
        return text
    while text and text_width(draw, text + "…", f) > max_w:
        text = text[:-1]
    return text.rstrip() + "…"


@dataclass
class Box:
    x0: int
    y0: int
    x1: int
    y1: int

    @property
    def w(self) -> int:
        return self.x1 - self.x0

    @property
    def h(self) -> int:
        return self.y1 - self.y0


@dataclass
class Context:
    """Per-render information shared with widgets."""
    now: datetime
    tz: ZoneInfo
    query: dict = field(default_factory=dict)  # query params from the Kindle (e.g. battery)
    screen: int = 0          # index of the screen being drawn
    screens: list = field(default_factory=list)  # names of all screens


LAYOUT_KEYS = {"widget", "height", "section"}


class Dashboard:
    """One or more screens; each screen is a vertical stack of widgets.

    The Kindle asks for ``/dashboard.png?screen=N``; N wraps around (and may be
    negative), so the Kindle can just count button presses up and down.
    """

    def __init__(self, config: dict):
        from .widgets import WIDGETS  # late import: widgets import drawing helpers from here

        self.config = config
        disp = config.get("display", {})
        self.tz = ZoneInfo(disp.get("timezone", "UTC"))
        self.panel = (int(disp.get("width", 600)), int(disp.get("height", 800)))
        self.landscape = disp.get("orientation", "portrait") == "landscape"

        screens = config.get("screens") or [{"name": "Dashboard", "layout": config.get("layout", [])}]
        self.screens = []  # [(name, [(widget, height), ...])]
        for n, scr in enumerate(screens):
            widgets = []
            for item in scr.get("layout", []):
                name = item["widget"]
                if name not in WIDGETS:
                    raise ValueError(f"unknown widget {name!r}; available: {', '.join(sorted(WIDGETS))}")
                # widget settings: its [section], overridden by extra keys on the layout entry
                section = dict(config.get(item.get("section", name), {}))
                section.update({k: v for k, v in item.items() if k not in LAYOUT_KEYS})
                widgets.append((WIDGETS[name](section), item.get("height")))
            self.screens.append((scr.get("name", f"Screen {n + 1}"), widgets))
        if not self.screens:
            raise ValueError("config has no [[screens]] (or [[layout]])")

    def screen_index(self, query: dict) -> int:
        try:
            return int(query.get("screen", 0)) % len(self.screens)
        except ValueError:
            return 0

    def render(self, query: dict | None = None) -> Image.Image:
        query = query or {}
        idx = self.screen_index(query)
        _, widgets = self.screens[idx]
        w, h = self.panel
        if self.landscape:
            w, h = h, w
        img = Image.new("L", (w, h), WHITE)
        draw = ImageDraw.Draw(img)
        ctx = Context(now=datetime.now(self.tz), tz=self.tz, query=query,
                      screen=idx, screens=[name for name, _ in self.screens])

        fixed = sum(hh for _, hh in widgets if hh)
        flex = max(0, h - fixed)
        y = 0
        for i, (widget, wh) in enumerate(widgets):
            wh = int(wh) if wh else flex
            box = Box(0, y, w, min(h, y + wh))
            t = time.monotonic()
            try:
                widget.draw(img, draw, box, ctx)
            except Exception as e:  # one broken widget must not blank the whole screen
                log.exception("widget %s failed", type(widget).__name__)
                draw.rectangle((box.x0, box.y0, box.x1 - 1, box.y1 - 1), fill=WHITE)
                draw.text((box.x0 + 20, box.y0 + 10), ellipsize(draw, f"{type(widget).__name__}: {e}", font(18), box.w - 40), font=font(18), fill=BLACK)
            log.debug("%s drawn in %.0f ms", type(widget).__name__, (time.monotonic() - t) * 1000)
            y += wh
            if i < len(widgets) - 1 and y < h:
                draw.line((20, y - 1, w - 20, y - 1), fill=MID, width=2)

        if self.landscape:
            img = img.rotate(90, expand=True)
        return img

    def render_png(self, query: dict | None = None) -> bytes:
        buf = io.BytesIO()
        # 8-bit greyscale, no alpha, no interlace: the format `eips -g` on the Kindle Keyboard accepts.
        self.render(query).save(buf, format="PNG", optimize=True)
        return buf.getvalue()
