from . import register
from ..render import BLACK, DARK, font


@register("clock")
class Clock:
    def __init__(self, cfg: dict):
        self.time_format = cfg.get("time_format", "%H:%M")
        self.date_format = cfg.get("date_format", "%A, %-d %B")

    def draw(self, img, draw, box, ctx):
        t = ctx.now.strftime(self.time_format)
        d = ctx.now.strftime(self.date_format)
        # Largest time font that fits the box, leaving room for the date line.
        date_f = font(max(20, box.h // 7))
        size = box.h
        while size > 20:
            f = font(size, bold=True)
            l, top, r, bottom = draw.textbbox((0, 0), t, font=f)
            if r - l <= box.w - 60 and bottom - top <= box.h - date_f.size - 50:
                break
            size -= 4
        l, top, r, bottom = draw.textbbox((0, 0), t, font=f)
        block_h = (bottom - top) + 20 + date_f.size
        y = box.y0 + (box.h - block_h) // 2
        draw.text((box.x0 + (box.w - (r - l)) // 2 - l, y - top), t, font=f, fill=BLACK)
        dw = draw.textlength(d, font=date_f)
        draw.text((box.x0 + (box.w - dw) // 2, y + (bottom - top) + 20), d, font=date_f, fill=DARK)
