"""xkcd comic (latest or random), scaled to fit its box."""
from __future__ import annotations

import io
import random
import textwrap

import requests
from PIL import Image, ImageOps

from . import Cached, register
from ..render import BLACK, DARK, font, ellipsize

UA = {"User-Agent": "kindle-dash/0.1"}


@register("xkcd")
class Xkcd:
    def __init__(self, cfg: dict):
        self.mode = cfg.get("mode", "random")  # "random" or "latest"
        self.show_title = cfg.get("show_title", True)
        self.show_alt = cfg.get("show_alt", False)
        self.max_upscale = float(cfg.get("max_upscale", 2.0))
        # random mode: prefer comics at least this wide relative to their height
        # (set to 0 to accept any shape, e.g. when xkcd has a big box of its own)
        self.min_aspect = float(cfg.get("min_aspect", 1.3))
        self.comic = Cached(self._fetch, float(cfg.get("refresh_minutes", 60)) * 60)

    def _get_image(self, info: dict) -> Image.Image:
        r = requests.get(info["img"], headers=UA, timeout=20)
        r.raise_for_status()
        pic = Image.open(io.BytesIO(r.content))
        if pic.mode in ("RGBA", "LA", "P"):
            pic = pic.convert("RGBA")
            pic = Image.alpha_composite(Image.new("RGBA", pic.size, "white"), pic)
        return pic.convert("L")

    def _fetch(self) -> dict:
        latest = requests.get("https://xkcd.com/info.0.json", headers=UA, timeout=15).json()
        info, pic = latest, None
        if self.mode == "random":
            # Some comics are interactive/missing, and tall ones would be tiny in a
            # short box, so try a few and keep the first that fits well.
            for _ in range(8):
                n = random.randint(1, latest["num"])
                r = requests.get(f"https://xkcd.com/{n}/info.0.json", headers=UA, timeout=15)
                if not (r.ok and r.json().get("img", "").lower().endswith((".png", ".jpg", ".gif"))):
                    continue
                info, pic = r.json(), self._get_image(r.json())
                if pic.width / pic.height >= self.min_aspect:
                    break
        if pic is None:
            pic = self._get_image(info)
        return {"num": info["num"], "title": info.get("safe_title", ""), "alt": info.get("alt", ""), "image": pic}

    def draw(self, img, draw, box, ctx):
        c = self.comic.get()
        pad = 16
        y0, y1 = box.y0 + pad, box.y1 - pad
        if self.show_title:
            tf = font(19, bold=True)
            title = ellipsize(draw, f"xkcd #{c['num']}: {c['title']}", tf, box.w - 2 * pad)
            draw.text((box.x0 + (box.w - draw.textlength(title, font=tf)) // 2, y0), title, font=tf, fill=BLACK)
            y0 += tf.size + 10
        alt_lines = []
        if self.show_alt and c["alt"]:
            af = font(14)
            chars = max(20, int((box.w - 2 * pad) / (af.size * 0.55)))
            alt_lines = textwrap.wrap(c["alt"], chars)[:3]
            y1 -= len(alt_lines) * (af.size + 4) + 6

        pic = c["image"]
        avail_w, avail_h = box.w - 2 * pad, max(10, y1 - y0)
        scale = min(avail_w / pic.width, avail_h / pic.height, self.max_upscale)
        size = (max(1, int(pic.width * scale)), max(1, int(pic.height * scale)))
        pic = pic.resize(size, Image.LANCZOS)
        pic = ImageOps.autocontrast(pic, cutoff=1)  # crisp black lines on e-ink
        img.paste(pic, (box.x0 + (box.w - size[0]) // 2, y0 + (avail_h - size[1]) // 2))

        if alt_lines:
            af = font(14)
            y = y1 + 6
            for line in alt_lines:
                draw.text((box.x0 + (box.w - draw.textlength(line, font=af)) // 2, y), line, font=af, fill=DARK)
                y += af.size + 4
