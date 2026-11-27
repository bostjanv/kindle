use anyhow::Result;
use toml::Table;

use super::Widget;
use crate::canvas::{BLACK, Bounds, Canvas, DARK, MID, font};
use crate::config;
use crate::render::Context;

/// Footer: screen indicator (left), last update + Kindle battery (right).
///
/// The battery comes from the Kindle as `?battery=NN`.
pub struct Status {
    show_screen_name: bool,
}

impl Status {
    pub fn new(cfg: &Table) -> Status {
        Status {
            show_screen_name: config::bool_or(cfg, "show_screen_name", true),
        }
    }
}

impl Widget for Status {
    fn name(&self) -> &'static str {
        "Status"
    }

    fn draw(&mut self, c: &mut Canvas, b: Bounds, ctx: &Context) -> Result<()> {
        let f = font(15, false);
        let cy = b.y0 + b.h() / 2;
        let text_y = (cy - f.size / 2 - 1) as f32;
        // screen dots: ● for the current screen, ○ for the others
        if ctx.screens.len() > 1 {
            let (mut x, r) = (b.x0 + 22, 5);
            for i in 0..ctx.screens.len() {
                let (x0, y0, x1, y1) = ((x - r) as f32, (cy - r) as f32, (x + r) as f32, (cy + r) as f32);
                if i == ctx.screen {
                    c.ellipse(x0, y0, x1, y1, Some(BLACK), None);
                } else {
                    c.ellipse(x0, y0, x1, y1, None, Some((MID, 2.0)));
                }
                x += 3 * r + 4;
            }
            if self.show_screen_name {
                c.text((x + 4) as f32, text_y, &ctx.screens[ctx.screen], &f, DARK);
            }
        }

        let mut parts = vec![format!("Updated {}", ctx.now.format("%H:%M"))];
        if let Some(battery) = ctx.query.get("battery")
            && !battery.is_empty()
            && battery.chars().all(|ch| ch.is_ascii_digit())
        {
            parts.push(format!("Battery {battery}%"));
        }
        let text = parts.join("  ·  ");
        let w = f.length(&text);
        c.text(b.x1 as f32 - 20.0 - w, text_y, &text, &f, DARK);
        Ok(())
    }
}
