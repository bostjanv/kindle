//! Layout: turns the configured screens (stacks of widgets) into a Kindle-ready PNG.

use std::collections::HashMap;
use std::time::Instant;

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use image::GrayImage;
use toml::{Table, Value};

use crate::canvas::{self, BLACK, Bounds, Canvas, MID, font};
use crate::config;
use crate::widgets::{self, Widget};

/// Per-render information shared with widgets.
pub struct Context {
    pub now: DateTime<Tz>,
    pub tz: Tz,
    /// Query parameters from the Kindle (e.g. battery).
    pub query: HashMap<String, String>,
    /// Index of the screen being drawn.
    pub screen: usize,
    /// Names of all screens.
    pub screens: Vec<String>,
}

/// Layout-entry keys that are not widget settings.
const LAYOUT_KEYS: [&str; 3] = ["widget", "height", "section"];

struct Screen {
    name: String,
    widgets: Vec<(Box<dyn Widget>, Option<i32>)>,
}

/// One or more screens; each screen is a vertical stack of widgets.
///
/// The Kindle asks for `/dashboard.png?screen=N`; N wraps around (and may be
/// negative), so the Kindle can just count button presses up and down.
pub struct Dashboard {
    tz: Tz,
    panel: (u32, u32),
    landscape: bool,
    screens: Vec<Screen>,
}

impl Dashboard {
    pub fn new(cfg: &Table) -> Result<Dashboard> {
        let empty = Table::new();
        let disp = config::table(cfg, "display").unwrap_or(&empty);
        let tz_name = config::str_or(disp, "timezone", "UTC");
        let tz: Tz = tz_name
            .parse()
            .map_err(|_| anyhow::anyhow!("unknown timezone {tz_name:?}"))?;
        let panel = (
            config::i64_or(disp, "width", 600) as u32,
            config::i64_or(disp, "height", 800) as u32,
        );
        let landscape = config::str_or(disp, "orientation", "portrait") == "landscape";

        let mut screen_cfgs: Vec<Table> = cfg
            .get("screens")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_table().cloned()).collect())
            .unwrap_or_default();
        if screen_cfgs.is_empty() {
            // legacy single-screen config: top-level [[layout]]
            let mut t = Table::new();
            t.insert("name".into(), "Dashboard".into());
            t.insert(
                "layout".into(),
                cfg.get("layout").cloned().unwrap_or(Value::Array(vec![])),
            );
            screen_cfgs.push(t);
        }

        let mut screens = Vec::new();
        for (n, scr) in screen_cfgs.iter().enumerate() {
            let mut list = Vec::new();
            for item in scr.get("layout").and_then(Value::as_array).into_iter().flatten() {
                let Some(item) = item.as_table() else {
                    bail!("layout entries must be tables")
                };
                let Some(name) = item.get("widget").and_then(Value::as_str) else {
                    bail!("layout entry without `widget`")
                };
                // widget settings: its [section], overridden by extra keys on the layout entry
                let section_name = item.get("section").and_then(Value::as_str).unwrap_or(name);
                let mut section = config::table(cfg, section_name).cloned().unwrap_or_default();
                for (k, v) in item {
                    if !LAYOUT_KEYS.contains(&k.as_str()) {
                        section.insert(k.clone(), v.clone());
                    }
                }
                let height = config::opt_f64(item, "height").map(|h| h as i32).filter(|&h| h != 0);
                list.push((widgets::create(name, &section)?, height));
            }
            let name = scr.get("name").and_then(Value::as_str).map(str::to_string);
            screens.push(Screen {
                name: name.unwrap_or_else(|| format!("Screen {}", n + 1)),
                widgets: list,
            });
        }
        Ok(Dashboard {
            tz,
            panel,
            landscape,
            screens,
        })
    }

    fn screen_index(&self, query: &HashMap<String, String>) -> usize {
        let n = query
            .get("screen")
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(0);
        n.rem_euclid(self.screens.len() as i64) as usize
    }

    pub fn render(&mut self, query: &HashMap<String, String>) -> GrayImage {
        let idx = self.screen_index(query);
        let (mut w, mut h) = self.panel;
        if self.landscape {
            (w, h) = (h, w);
        }
        let (w, h) = (w as i32, h as i32);
        let mut c = Canvas::new(w as u32, h as u32);
        let ctx = Context {
            now: Utc::now().with_timezone(&self.tz),
            tz: self.tz,
            query: query.clone(),
            screen: idx,
            screens: self.screens.iter().map(|s| s.name.clone()).collect(),
        };

        let widgets = &mut self.screens[idx].widgets;
        let fixed: i32 = widgets.iter().filter_map(|(_, wh)| *wh).sum();
        let flex = (h - fixed).max(0);
        let count = widgets.len();
        let mut y = 0;
        for (i, (widget, wh)) in widgets.iter_mut().enumerate() {
            let wh = wh.unwrap_or(flex);
            let b = Bounds {
                x0: 0,
                y0: y,
                x1: w,
                y1: h.min(y + wh),
            };
            let t = Instant::now();
            if let Err(e) = widget.draw(&mut c, b, &ctx) {
                // one broken widget must not blank the whole screen
                log::error!("widget {} failed: {e:#}", widget.name());
                c.rect(
                    b.x0 as f32,
                    b.y0 as f32,
                    (b.x1 - 1) as f32,
                    (b.y1 - 1) as f32,
                    canvas::WHITE,
                );
                let f = font(18, false);
                let msg = f.ellipsize(&format!("{}: {e:#}", widget.name()), b.w() - 40);
                c.text((b.x0 + 20) as f32, (b.y0 + 10) as f32, &msg, &f, BLACK);
            }
            log::debug!(
                "{} drawn in {:.0} ms",
                widget.name(),
                t.elapsed().as_secs_f64() * 1000.0
            );
            y += wh;
            if i < count - 1 && y < h {
                c.line(20.0, (y - 1) as f32, (w - 20) as f32, (y - 1) as f32, MID, 2.0);
            }
        }

        let img = c.into_gray();
        if self.landscape {
            image::imageops::rotate270(&img)
        } else {
            img
        }
    }

    pub fn render_png(&mut self, query: &HashMap<String, String>) -> Result<Vec<u8>> {
        canvas::encode_png(&self.render(query))
    }
}
