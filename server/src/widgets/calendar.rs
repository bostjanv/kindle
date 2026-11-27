//! Upcoming events from iCalendar (.ics) sources and Remind files.
//!
//! Remind files are read with `remind -ppp`, so the `remind` program must be on PATH.

use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, Days, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;
use toml::{Table, Value};

use super::Widget;
use crate::cache::Cached;
use crate::canvas::{BLACK, Bounds, Canvas, DARK, LIGHT, MID, font};
use crate::ics::{self, Occurrence};
use crate::render::Context;
use crate::{config, http, remind};

enum Kind {
    Ics(String),
    Remind { path: PathBuf, zone: Option<Tz> },
}

enum Data {
    Ics(ics::Calendar),
    /// Remind output: already expanded occurrences.
    Fixed(Vec<Occurrence>),
}

struct Source {
    label: String,
    kind: Kind,
    cache: Cached<Data>,
}

pub fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(rest),
        None => PathBuf::from(p),
    }
}

fn load(src: &str) -> Result<Vec<u8>> {
    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("webcal://") {
        return http::get_bytes(&src.replacen("webcal://", "https://", 1), 20);
    }
    let path = expand_home(src);
    std::fs::read(&path).map_err(|e| anyhow!("{}: {e}", path.display()))
}

struct Item {
    day: NaiveDate,
    all_day: bool,
    start: DateTime<Tz>,
    title: String,
}

pub struct Calendar {
    sources: Vec<Source>,
    days: i64,
}

impl Calendar {
    pub fn new(cfg: &Table) -> Result<Calendar> {
        let ttl = config::f64_or(cfg, "refresh_minutes", 10.0) * 60.0;
        let mut sources = Vec::new();
        for s in cfg.get("sources").and_then(Value::as_array).into_iter().flatten() {
            let t = match s {
                Value::String(u) => Table::from_iter([("url".to_string(), Value::String(u.clone()))]),
                Value::Table(t) => t.clone(),
                _ => bail!("[calendar] sources must be strings or tables"),
            };
            let get = |k: &str| t.get(k).and_then(Value::as_str);
            let kind = if let Some(r) = get("remind") {
                let zone = get("timezone")
                    .map(|z| z.parse().map_err(|_| anyhow!("unknown timezone {z:?}")))
                    .transpose()?;
                Kind::Remind {
                    path: expand_home(r),
                    zone,
                }
            } else if let Some(u) = get("url") {
                Kind::Ics(u.to_string())
            } else {
                bail!("calendar source needs `url` or `remind`");
            };
            let label = get("name")
                .or(get("url"))
                .or(get("remind"))
                .unwrap_or("calendar")
                .to_string();
            sources.push(Source {
                label,
                kind,
                cache: Cached::new(ttl),
            });
        }
        Ok(Calendar {
            sources,
            days: config::i64_or(cfg, "days_ahead", 7),
        })
    }

    fn fetch(kind: &Kind, ctx: &Context, days: i64) -> Result<Data> {
        Ok(match kind {
            Kind::Ics(u) => Data::Ics(ics::Calendar::parse(&load(u)?)?),
            Kind::Remind { path, zone } => {
                // Start a day early so events crossing midnight are kept; months cover days_ahead.
                let start = ctx.now.date_naive() - Days::new(1);
                let months = (days.max(0) / 28 + 2) as u32;
                Data::Fixed(remind::load(path, start, months, zone.unwrap_or(ctx.tz), ctx.tz)?)
            }
        })
    }

    fn events(&mut self, ctx: &Context) -> (Vec<Item>, Vec<String>) {
        let today = ctx.now.date_naive();
        let midnight = |d: NaiveDate| ctx.tz.from_local_datetime(&d.and_time(NaiveTime::MIN)).earliest();
        let start = midnight(today).unwrap_or(ctx.now);
        let end = midnight(today + Days::new(self.days.max(0) as u64)).unwrap_or(ctx.now);
        let (mut out, mut errors) = (Vec::new(), Vec::new());
        let days = self.days;
        for Source { label, kind, cache } in &mut self.sources {
            let data = match cache.get(|| Self::fetch(kind, ctx, days)) {
                Ok(d) => d,
                Err(e) => {
                    errors.push(format!("{label}: {e:#}"));
                    continue;
                }
            };
            let occurrences = match data {
                Data::Ics(cal) => cal.between(start, end, ctx.tz),
                Data::Fixed(v) => v.iter().filter(|o| o.overlaps(start, end)).cloned().collect(),
            };
            if cache.error.is_some() {
                errors.push(format!("{label} stale"));
            }
            for o in occurrences {
                if !o.all_day && o.end < ctx.now && o.end > o.start {
                    continue; // already finished today
                }
                // multi-day events: show on the first visible day
                let day = o.start.date_naive().max(today);
                out.push(Item {
                    day,
                    all_day: o.all_day,
                    start: o.start,
                    title: o.summary,
                });
            }
        }
        out.sort_by_key(|e| (e.day, !e.all_day, e.start));
        (out, errors)
    }
}

impl Widget for Calendar {
    fn name(&self) -> &'static str {
        "Calendar"
    }

    fn draw(&mut self, c: &mut Canvas, b: Bounds, ctx: &Context) -> Result<()> {
        let pad = 24;
        let (hf, ef, tf) = (font(17, true), font(21, false), font(19, true));
        let mut y = b.y0 + 16;
        let x = (b.x0 + pad) as f32;
        if self.sources.is_empty() {
            let msg = "No calendars configured (see [calendar] in config.toml)";
            c.text(x, y as f32, msg, &font(18, false), DARK);
            return Ok(());
        }
        let (events, errors) = self.events(ctx);
        let today = ctx.now.date_naive();
        let mut current_day = None;
        let line_h = ef.size + 10;
        for (i, ev) in events.iter().enumerate() {
            let need = line_h + if Some(ev.day) != current_day { hf.size + 10 } else { 0 };
            if y + need > b.y1 - if i < events.len() - 1 { 24 } else { 0 } {
                c.text(
                    x,
                    (y + 2) as f32,
                    &format!("+ {} more", events.len() - i),
                    &font(17, false),
                    MID,
                );
                break;
            }
            if Some(ev.day) != current_day {
                current_day = Some(ev.day);
                let label = if ev.day == today {
                    "TODAY".to_string()
                } else if Some(ev.day) == today.succ_opt() {
                    "TOMORROW".to_string()
                } else {
                    ev.day.format("%A %-d %b").to_string().to_uppercase()
                };
                c.text(x, y as f32, &label, &hf, DARK);
                let ly = (y + hf.size / 2 + 2) as f32;
                c.line(x + hf.length(&label) + 12.0, ly, (b.x1 - pad) as f32, ly, LIGHT, 1.0);
                y += hf.size + 10;
            }
            if ev.all_day {
                c.rounded_rect_outline(x, (y + 2) as f32, x + 76.0, (y + ef.size + 4) as f32, 4.0, BLACK, 2.0);
                let sf = font(15, false);
                c.text(
                    x + 38.0 - sf.length("all day") / 2.0,
                    (y + 4) as f32,
                    "all day",
                    &sf,
                    BLACK,
                );
            } else {
                c.text(x, y as f32, &ev.start.format("%H:%M").to_string(), &tf, BLACK);
            }
            let x_title = b.x0 + pad + 96;
            c.text(
                x_title as f32,
                y as f32,
                &ef.ellipsize(&ev.title, b.x1 - pad - x_title),
                &ef,
                BLACK,
            );
            y += line_h;
        }
        if events.is_empty() {
            let msg = format!("Nothing scheduled in the next {} days", self.days);
            c.text(x, y as f32, &msg, &font(20, false), DARK);
        }
        if !errors.is_empty() {
            let f = font(13, false);
            let msg = f.ellipsize(&format!("⚠ {}", errors.join("; ")), b.w() - 2 * pad);
            c.text(x, (b.y1 - f.size - 6) as f32, &msg, &f, MID);
        }
        Ok(())
    }
}
