from . import register
from ..render import BLACK, DARK, MID, font


@register("status")
class Status:
    """Footer: screen indicator (left), last update + Kindle battery (right).

    The battery comes from the Kindle as ?battery=NN.
    """

    def __init__(self, cfg: dict):
        self.show_screen_name = cfg.get("show_screen_name", True)

    def draw(self, img, draw, box, ctx):
        f = font(15)
        cy = box.y0 + box.h // 2
        # screen dots: ● for the current screen, ○ for the others
        if len(ctx.screens) > 1:
            x, r = box.x0 + 22, 5
            for i in range(len(ctx.screens)):
                bbox = (x - r, cy - r, x + r, cy + r)
                if i == ctx.screen:
                    draw.ellipse(bbox, fill=BLACK)
                else:
                    draw.ellipse(bbox, outline=MID, width=2)
                x += 3 * r + 4
            if self.show_screen_name:
                draw.text((x + 4, cy - f.size // 2 - 1), ctx.screens[ctx.screen], font=f, fill=DARK)

        parts = [f"Updated {ctx.now:%H:%M}"]
        battery = ctx.query.get("battery")
        if battery and battery.isdigit():
            parts.append(f"Battery {battery}%")
        text = "  ·  ".join(parts)
        w = draw.textlength(text, font=f)
        draw.text((box.x1 - 20 - w, cy - f.size // 2 - 1), text, font=f, fill=DARK)
