"""Upcoming events from one or more iCalendar (.ics) sources."""
from __future__ import annotations

from datetime import date, datetime, time, timedelta
from pathlib import Path

import icalendar
import recurring_ical_events
import requests

from . import Cached, register
from ..render import BLACK, DARK, MID, LIGHT, font, ellipsize


def _load(src: str) -> bytes:
    if src.startswith(("http://", "https://", "webcal://")):
        r = requests.get(src.replace("webcal://", "https://", 1), timeout=20)
        r.raise_for_status()
        return r.content
    return Path(src).expanduser().read_bytes()


@register("calendar")
class Calendar:
    def __init__(self, cfg: dict):
        self.sources = [s if isinstance(s, dict) else {"url": s} for s in cfg.get("sources", [])]
        self.days = int(cfg.get("days_ahead", 7))
        self.ttl = float(cfg.get("refresh_minutes", 10)) * 60
        self.cals = [Cached(lambda u=s["url"]: icalendar.Calendar.from_ical(_load(u)), self.ttl) for s in self.sources]

    def _events(self, ctx):
        start = datetime.combine(ctx.now.date(), time.min, ctx.tz)
        end = start + timedelta(days=self.days)
        out, errors = [], []
        for src, cache in zip(self.sources, self.cals):
            try:
                cal = cache.get()
            except Exception as e:
                errors.append(f"{src.get('name', src['url'])}: {e}")
                continue
            if cache.error:
                errors.append(f"{src.get('name', 'calendar')} stale")
            for ev in recurring_ical_events.of(cal).between(start, end):
                s = ev.get("DTSTART").dt
                e = ev.get("DTEND").dt if ev.get("DTEND") else s
                all_day = not isinstance(s, datetime)
                if all_day:
                    s_local = datetime.combine(s, time.min, ctx.tz)
                    e_local = datetime.combine(e, time.min, ctx.tz)
                else:
                    s_local = (s if s.tzinfo else s.replace(tzinfo=ctx.tz)).astimezone(ctx.tz)
                    e_local = (e if isinstance(e, datetime) and e.tzinfo else s_local).astimezone(ctx.tz) if isinstance(e, datetime) else s_local
                if not all_day and e_local < ctx.now and e_local > s_local:
                    continue  # already finished today
                # multi-day all-day events: show on the first visible day
                day = max(s_local.date(), ctx.now.date())
                out.append((day, all_day, s_local, e_local, str(ev.get("SUMMARY", "(no title)"))))
        out.sort(key=lambda x: (x[0], not x[1], x[2]))
        return out, errors

    def draw(self, img, draw, box, ctx):
        pad = 24
        hf = font(17, bold=True)
        ef = font(21)
        tf = font(19, bold=True)
        y = box.y0 + 16
        if not self.sources:
            draw.text((box.x0 + pad, y), "No calendars configured (see [calendar] in config.toml)", font=font(18), fill=DARK)
            return
        events, errors = self._events(ctx)
        today = ctx.now.date()
        current_day = None
        line_h = ef.size + 10
        shown = 0
        for i, (day, all_day, s, e, title) in enumerate(events):
            need = line_h + (hf.size + 10 if day != current_day else 0)
            if y + need > box.y1 - (24 if i < len(events) - 1 else 0):
                more = len(events) - shown
                draw.text((box.x0 + pad, y + 2), f"+ {more} more", font=font(17), fill=MID)
                break
            if day != current_day:
                current_day = day
                label = "TODAY" if day == today else "TOMORROW" if day == today + timedelta(days=1) else day.strftime("%A %-d %b").upper()
                draw.text((box.x0 + pad, y), label, font=hf, fill=DARK)
                lw = draw.textlength(label, font=hf)
                draw.line((box.x0 + pad + lw + 12, y + hf.size // 2 + 2, box.x1 - pad, y + hf.size // 2 + 2), fill=LIGHT, width=1)
                y += hf.size + 10
            tx = box.x0 + pad
            if all_day:
                draw.rounded_rectangle((tx, y + 2, tx + 76, y + ef.size + 4), radius=4, outline=BLACK, width=2)
                draw.text((tx + 38 - draw.textlength("all day", font=font(15)) / 2, y + 4), "all day", font=font(15), fill=BLACK)
            else:
                draw.text((tx, y), s.strftime("%H:%M"), font=tf, fill=BLACK)
            x_title = tx + 96
            draw.text((x_title, y), ellipsize(draw, title, ef, box.x1 - pad - x_title), font=ef, fill=BLACK)
            y += line_h
            shown += 1
        if not events:
            draw.text((box.x0 + pad, y), f"Nothing scheduled in the next {self.days} days", font=font(20), fill=DARK)
        if errors:
            f = font(13)
            msg = ellipsize(draw, "⚠ " + "; ".join(errors), f, box.w - 2 * pad)
            draw.text((box.x0 + pad, box.y1 - f.size - 6), msg, font=f, fill=MID)
