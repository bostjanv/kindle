use anyhow::Result;
use toml::Table;

use super::{Widget, check_format};
use crate::canvas::{BLACK, Bounds, Canvas, DARK, font};
use crate::config;
use crate::render::Context;

/// Big centred time + date; in a short box (< 130 px) a single compact line.
pub struct Clock {
    time_format: String,
    date_format: String,
}

impl Clock {
    pub fn new(cfg: &Table) -> Result<Clock> {
        Ok(Clock {
            time_format: check_format(&config::str_or(cfg, "time_format", "%H:%M"))?,
            date_format: check_format(&config::str_or(cfg, "date_format", "%A, %-d %B"))?,
        })
    }

    fn compact(&self, c: &mut Canvas, b: Bounds, t: &str, d: &str) {
        let pad = 24;
        let tf = font(20.max((b.h() as f32 * 0.62) as i32), true);
        let (l, top, _, bottom) = tf.bbox(t);
        c.text(
            (b.x0 + pad - l) as f32,
            (b.y0 + (b.h() - (bottom - top)).div_euclid(2) - top) as f32,
            t,
            &tf,
            BLACK,
        );
        let df = font(16.max((b.h() as f32 * 0.26) as i32), false);
        let dw = df.length(d);
        c.text(
            b.x1 as f32 - pad as f32 - dw,
            (b.y0 + (b.h() - df.size).div_euclid(2)) as f32,
            d,
            &df,
            DARK,
        );
    }
}

impl Widget for Clock {
    fn name(&self) -> &'static str {
        "Clock"
    }

    fn draw(&mut self, c: &mut Canvas, b: Bounds, ctx: &Context) -> Result<()> {
        let t = ctx.now.format(&self.time_format).to_string();
        let d = ctx.now.format(&self.date_format).to_string();
        if b.h() < 130 {
            self.compact(c, b, &t, &d);
            return Ok(());
        }
        // Largest time font that fits the box, leaving room for the date line.
        let date_f = font(20.max(b.h() / 7), false);
        let mut size = b.h();
        let mut f = font(size, true);
        while size > 20 {
            f = font(size, true);
            let (l, top, r, bottom) = f.bbox(&t);
            if r - l <= b.w() - 60 && bottom - top <= b.h() - date_f.size - 50 {
                break;
            }
            size -= 4;
        }
        let (l, top, r, bottom) = f.bbox(&t);
        let block_h = (bottom - top) + 20 + date_f.size;
        let y = b.y0 + (b.h() - block_h).div_euclid(2);
        c.text(
            (b.x0 + (b.w() - (r - l)).div_euclid(2) - l) as f32,
            (y - top) as f32,
            &t,
            &f,
            BLACK,
        );
        let dw = date_f.length(&d);
        c.text(
            b.x0 as f32 + (b.w() as f32 - dw) / 2.0,
            (y + (bottom - top) + 20) as f32,
            &d,
            &date_f,
            DARK,
        );
        Ok(())
    }
}
