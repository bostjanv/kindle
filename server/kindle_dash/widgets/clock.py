from . import register
from ..render import BLACK, DARK, font


@register("clock")
class Clock:
    """Big centred time + date; in a short box (< 130 px) a single compact line."""

    def __init__(self, cfg: dict):
        self.time_format = cfg.get("time_format", "%H:%M")
        self.date_format = cfg.get("date_format", "%A, %-d %B")

    def draw(self, img, draw, box, ctx):
        t = ctx.now.strftime(self.time_format)
        d = ctx.now.strftime(self.date_format)
        if box.h < 130:
            return self._compact(draw, box, t, d)
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

    def _compact(self, draw, box, t, d):
        pad = 24
        tf = font(max(20, int(box.h * 0.62)), bold=True)
        l, top, r, bottom = draw.textbbox((0, 0), t, font=tf)
        draw.text((box.x0 + pad - l, box.y0 + (box.h - (bottom - top)) // 2 - top), t, font=tf, fill=BLACK)
        df = font(max(16, int(box.h * 0.26)))
        dw = draw.textlength(d, font=df)
        draw.text((box.x1 - pad - dw, box.y0 + (box.h - df.size) // 2), d, font=df, fill=DARK)
