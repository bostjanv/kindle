"""Current conditions + daily forecast from Open-Meteo (free, no API key)."""
from __future__ import annotations

from datetime import date

import requests

from . import Cached, register
from .. import icons
from ..render import BLACK, DARK, MID, font, ellipsize

API = "https://api.open-meteo.com/v1/forecast"

WMO = {
    0: "Clear", 1: "Mainly clear", 2: "Partly cloudy", 3: "Overcast",
    45: "Fog", 48: "Freezing fog",
    51: "Light drizzle", 53: "Drizzle", 55: "Heavy drizzle", 56: "Freezing drizzle", 57: "Freezing drizzle",
    61: "Light rain", 63: "Rain", 65: "Heavy rain", 66: "Freezing rain", 67: "Freezing rain",
    71: "Light snow", 73: "Snow", 75: "Heavy snow", 77: "Snow grains",
    80: "Rain showers", 81: "Rain showers", 82: "Violent showers",
    85: "Snow showers", 86: "Snow showers",
    95: "Thunderstorm", 96: "Thunderstorm, hail", 99: "Thunderstorm, hail",
}


@register("weather")
class Weather:
    def __init__(self, cfg: dict):
        self.name = cfg.get("location_name", "")
        self.lat, self.lon = float(cfg["latitude"]), float(cfg["longitude"])
        self.imperial = cfg.get("units", "metric") == "imperial"
        self.days = int(cfg.get("forecast_days", 4))
        self.data = Cached(self._fetch, float(cfg.get("refresh_minutes", 15)) * 60)

    def _fetch(self) -> dict:
        params = {
            "latitude": self.lat, "longitude": self.lon, "timezone": "auto",
            "forecast_days": self.days + 1,
            "current": "temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day",
            "daily": "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset",
        }
        if self.imperial:
            params |= {"temperature_unit": "fahrenheit", "wind_speed_unit": "mph"}
        r = requests.get(API, params=params, timeout=15)
        r.raise_for_status()
        return r.json()

    def draw(self, img, draw, box, ctx):
        d = self.data.get()
        cur, daily = d["current"], d["daily"]
        wind_unit = "mph" if self.imperial else "km/h"
        pad = 24

        # --- current conditions (top ~55%) ---
        top_h = box.h - 132  # the forecast row needs ~132 px
        icon_size = min(top_h - 20, 140)
        icons.draw_icon(draw, cur["weather_code"], box.x0 + pad, box.y0 + (top_h - icon_size) // 2 + 4,
                        icon_size, night=not cur.get("is_day", 1))
        tx = box.x0 + pad + icon_size + 24
        temp = f"{round(cur['temperature_2m'])}°"
        tf = font(min(96, top_h - 40), bold=True)
        l, t, r, b = draw.textbbox((0, 0), temp, font=tf)
        ty = box.y0 + (top_h - (b - t)) // 2 - t
        draw.text((tx, ty), temp, font=tf, fill=BLACK)

        rx = tx + (r - l) + 28
        lines = [
            (WMO.get(cur["weather_code"], "—"), font(24, bold=True), BLACK),
            (f"Feels like {round(cur['apparent_temperature'])}°", font(19), DARK),
            (f"Wind {round(cur['wind_speed_10m'])} {wind_unit} · RH {cur['relative_humidity_2m']}%", font(19), DARK),
            (f"↑ {daily['sunrise'][0][-5:]}   ↓ {daily['sunset'][0][-5:]}", font(19), DARK),
        ]
        if self.name:
            lines.insert(0, (self.name, font(17), MID))
        total = sum(f.size + 8 for _, f, _ in lines)
        y = box.y0 + (top_h - total) // 2
        for text, f, fill in lines:
            draw.text((rx, y), ellipsize(draw, text, f, box.x1 - pad - rx), font=f, fill=fill)
            y += f.size + 8

        # --- daily forecast row ---
        n = min(self.days, len(daily["time"]) - 1)
        if n <= 0:
            return
        y0 = box.y0 + top_h
        col_w = (box.w - 2 * pad) // n
        for i in range(n):
            j = i + 1  # skip today, it's shown above
            cx = box.x0 + pad + i * col_w + col_w // 2
            day = date.fromisoformat(daily["time"][j]).strftime("%a")
            df = font(19, bold=True)
            draw.text((cx - draw.textlength(day, font=df) / 2, y0 + 6), day, font=df, fill=BLACK)
            isz = 46
            icons.draw_icon(draw, daily["weather_code"][j], cx - isz // 2, y0 + 32, isz)
            hi_lo = f"{round(daily['temperature_2m_max'][j])}° / {round(daily['temperature_2m_min'][j])}°"
            f = font(18)
            draw.text((cx - draw.textlength(hi_lo, font=f) / 2, y0 + 84), hi_lo, font=f, fill=BLACK)
            pp = daily.get("precipitation_probability_max", [None] * (j + 1))[j]
            if pp is not None:
                s = f"☂ {pp}%"
                f2 = font(15)
                draw.text((cx - draw.textlength(s, font=f2) / 2, y0 + 106), s, font=f2, fill=DARK)

        if self.data.error:  # serving stale data: say so, subtly
            f = font(13)
            draw.text((box.x1 - 120, box.y0 + 4), "⚠ weather stale", font=f, fill=MID)
