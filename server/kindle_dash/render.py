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


class Dashboard:
    def __init__(self, config: dict):
        from .widgets import WIDGETS  # late import: widgets import drawing helpers from here

        self.config = config
        disp = config.get("display", {})
        self.tz = ZoneInfo(disp.get("timezone", "UTC"))
        self.panel = (int(disp.get("width", 600)), int(disp.get("height", 800)))
        self.landscape = disp.get("orientation", "portrait") == "landscape"
        self.widgets = []
        for item in config.get("layout", []):
            name = item["widget"]
            if name not in WIDGETS:
                raise ValueError(f"unknown widget {name!r}; available: {', '.join(sorted(WIDGETS))}")
            self.widgets.append((WIDGETS[name](config.get(name, {})), item.get("height")))

    def render(self, query: dict | None = None) -> Image.Image:
        w, h = self.panel
        if self.landscape:
            w, h = h, w
        img = Image.new("L", (w, h), WHITE)
        draw = ImageDraw.Draw(img)
        ctx = Context(now=datetime.now(self.tz), tz=self.tz, query=query or {})

        fixed = sum(hh for _, hh in self.widgets if hh)
        flex = max(0, h - fixed)
        y = 0
        for i, (widget, wh) in enumerate(self.widgets):
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
            if i < len(self.widgets) - 1 and y < h:
                draw.line((20, y - 1, w - 20, y - 1), fill=MID, width=2)

        if self.landscape:
            img = img.rotate(90, expand=True)
        return img

    def render_png(self, query: dict | None = None) -> bytes:
        buf = io.BytesIO()
        # 8-bit greyscale, no alpha, no interlace: the format `eips -g` on the Kindle Keyboard accepts.
        self.render(query).save(buf, format="PNG", optimize=True)
        return buf.getvalue()
