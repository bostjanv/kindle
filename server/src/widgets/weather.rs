//! Current conditions + daily forecast from Open-Meteo (free, no API key).

use anyhow::{Context as _, Result, anyhow};
use chrono::NaiveDate;
use serde_json::Value;
use toml::Table;

use super::Widget;
use crate::cache::Cached;
use crate::canvas::{BLACK, Bounds, Canvas, DARK, MID, font};
use crate::render::Context;
use crate::{config, http, icons};

const API: &str = "https://api.open-meteo.com/v1/forecast";

fn wmo(code: i64) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 => "Fog",
        48 => "Freezing fog",
        51 => "Light drizzle",
        53 => "Drizzle",
        55 => "Heavy drizzle",
        56 | 57 => "Freezing drizzle",
        61 => "Light rain",
        63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71 => "Light snow",
        73 => "Snow",
        75 => "Heavy snow",
        77 => "Snow grains",
        80 | 81 => "Rain showers",
        82 => "Violent showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm, hail",
        _ => "—",
    }
}

/// Python's `round()`: ties to even.
fn round(v: f64) -> i64 {
    v.round_ties_even() as i64
}

fn num(v: &Value, what: &str) -> Result<f64> {
    v.as_f64().ok_or_else(|| anyhow!("weather: missing {what}"))
}

/// A JSON number as Python would print it (`85`, `85.5`).
fn show(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

fn hhmm(v: &Value) -> String {
    let s = v.as_str().unwrap_or("");
    s.get(s.len().saturating_sub(5)..).unwrap_or("").to_string()
}

pub struct Weather {
    name: String,
    lat: f64,
    lon: f64,
    imperial: bool,
    days: i64,
    data: Cached<Value>,
}

impl Weather {
    pub fn new(cfg: &Table) -> Result<Weather> {
        Ok(Weather {
            name: config::str_or(cfg, "location_name", ""),
            lat: config::opt_f64(cfg, "latitude").context("[weather] needs latitude")?,
            lon: config::opt_f64(cfg, "longitude").context("[weather] needs longitude")?,
            imperial: config::str_or(cfg, "units", "metric") == "imperial",
            days: config::i64_or(cfg, "forecast_days", 4),
            data: Cached::new(config::f64_or(cfg, "refresh_minutes", 15.0) * 60.0),
        })
    }

    fn fetch(lat: f64, lon: f64, days: i64, imperial: bool) -> Result<Value> {
        let mut q = vec![
            ("latitude", lat.to_string()),
            ("longitude", lon.to_string()),
            ("timezone", "auto".into()),
            ("forecast_days", (days + 1).to_string()),
            (
                "current",
                "temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day".into(),
            ),
            (
                "daily",
                "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset"
                    .into(),
            ),
        ];
        if imperial {
            q.push(("temperature_unit", "fahrenheit".into()));
            q.push(("wind_speed_unit", "mph".into()));
        }
        http::get_json(API, &q, 15)
    }
}

impl Widget for Weather {
    fn name(&self) -> &'static str {
        "Weather"
    }

    fn draw(&mut self, c: &mut Canvas, b: Bounds, _ctx: &Context) -> Result<()> {
        let (lat, lon, days, imperial) = (self.lat, self.lon, self.days, self.imperial);
        let d = self.data.get(|| Self::fetch(lat, lon, days, imperial))?;
        let (cur, daily) = (&d["current"], &d["daily"]);
        let wind_unit = if self.imperial { "mph" } else { "km/h" };
        let pad = 24;

        // --- current conditions (top ~55%) ---
        let top_h = b.h() - 132; // the forecast row needs ~132 px
        let icon_size = (top_h - 20).min(140);
        let code = cur["weather_code"].as_i64().unwrap_or(-1);
        let night = cur["is_day"].as_i64() == Some(0);
        icons::draw_icon(
            c,
            code,
            b.x0 + pad,
            b.y0 + (top_h - icon_size).div_euclid(2) + 4,
            icon_size,
            night,
        );
        let tx = b.x0 + pad + icon_size + 24;
        let temp = format!("{}°", round(num(&cur["temperature_2m"], "temperature")?));
        let tf = font(96.min(top_h - 40), true);
        let (l, t, r, bt) = tf.bbox(&temp);
        let ty = b.y0 + (top_h - (bt - t)).div_euclid(2) - t;
        c.text(tx as f32, ty as f32, &temp, &tf, BLACK);

        let rx = tx + (r - l) + 28;
        let mut lines = vec![
            (wmo(code).to_string(), font(24, true), BLACK),
            (
                format!(
                    "Feels like {}°",
                    round(num(&cur["apparent_temperature"], "apparent temperature")?)
                ),
                font(19, false),
                DARK,
            ),
            (
                format!(
                    "Wind {} {wind_unit} · RH {}%",
                    round(num(&cur["wind_speed_10m"], "wind")?),
                    show(&cur["relative_humidity_2m"])
                ),
                font(19, false),
                DARK,
            ),
            (
                format!("↑ {}   ↓ {}", hhmm(&daily["sunrise"][0]), hhmm(&daily["sunset"][0])),
                font(19, false),
                DARK,
            ),
        ];
        if !self.name.is_empty() {
            lines.insert(0, (self.name.clone(), font(17, false), MID));
        }
        let total: i32 = lines.iter().map(|(_, f, _)| f.size + 8).sum();
        let mut y = b.y0 + (top_h - total).div_euclid(2);
        for (text, f, fill) in &lines {
            c.text(rx as f32, y as f32, &f.ellipsize(text, b.x1 - pad - rx), f, *fill);
            y += f.size + 8;
        }

        // --- daily forecast row ---
        let times = daily["time"].as_array().map(Vec::len).unwrap_or(0) as i64;
        let n = self.days.min(times - 1);
        if n > 0 {
            let y0 = b.y0 + top_h;
            let col_w = (b.w() - 2 * pad).div_euclid(n as i32);
            for i in 0..n as usize {
                let j = i + 1; // skip today, it's shown above
                let cx = (b.x0 + pad + i as i32 * col_w + col_w / 2) as f32;
                let day = daily["time"][j]
                    .as_str()
                    .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                    .map(|d| d.format("%a").to_string())
                    .unwrap_or_default();
                let df = font(19, true);
                c.text(cx - df.length(&day) / 2.0, (y0 + 6) as f32, &day, &df, BLACK);
                let isz = 46;
                let dcode = daily["weather_code"][j].as_i64().unwrap_or(-1);
                icons::draw_icon(c, dcode, cx as i32 - isz / 2, y0 + 32, isz, false);
                let hi_lo = format!(
                    "{}° / {}°",
                    round(num(&daily["temperature_2m_max"][j], "max temperature")?),
                    round(num(&daily["temperature_2m_min"][j], "min temperature")?)
                );
                let f = font(18, false);
                c.text(cx - f.length(&hi_lo) / 2.0, (y0 + 84) as f32, &hi_lo, &f, BLACK);
                let pp = &daily["precipitation_probability_max"][j];
                if !pp.is_null() {
                    let s = format!("☂ {}%", show(pp));
                    let f2 = font(15, false);
                    c.text(cx - f2.length(&s) / 2.0, (y0 + 106) as f32, &s, &f2, DARK);
                }
            }
        }

        if self.data.error.is_some() {
            // serving stale data: say so, subtly
            c.text(
                (b.x1 - 120) as f32,
                (b.y0 + 4) as f32,
                "⚠ weather stale",
                &font(13, false),
                MID,
            );
        }
        Ok(())
    }
}
