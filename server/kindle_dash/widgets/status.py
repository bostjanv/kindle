from . import register
from ..render import DARK, font


@register("status")
class Status:
    """Small footer: last update time and the Kindle's battery (sent as ?battery=NN)."""

    def __init__(self, cfg: dict):
        pass

    def draw(self, img, draw, box, ctx):
        f = font(15)
        parts = [f"Updated {ctx.now:%H:%M}"]
        battery = ctx.query.get("battery")
        if battery and battery.isdigit():
            parts.append(f"Battery {battery}%")
        text = "  ·  ".join(parts)
        w = draw.textlength(text, font=f)
        draw.text((box.x1 - 20 - w, box.y0 + (box.h - f.size) // 2), text, font=f, fill=DARK)
