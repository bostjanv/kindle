//! Weather icons drawn with primitives so they stay crisp on e-ink at any size.

use std::f32::consts::PI;

use crate::canvas::{BLACK, Canvas, DARK, LIGHT, WHITE};

fn sun(d: &mut Canvas, cx: f32, cy: f32, r: f32, w: f32) {
    d.ellipse(cx - r, cy - r, cx + r, cy + r, None, Some((BLACK, w)));
    for k in 0..8 {
        let a = k as f32 * PI / 4.0;
        let (c, s) = (a.cos(), a.sin());
        d.line(
            cx + c * r * 1.4,
            cy + s * r * 1.4,
            cx + c * r * 1.85,
            cy + s * r * 1.85,
            BLACK,
            w,
        );
    }
}

fn moon(d: &mut Canvas, cx: f32, cy: f32, r: f32, _w: f32) {
    d.ellipse(cx - r, cy - r, cx + r, cy + r, Some(BLACK), None);
    let o = r * 0.55;
    d.ellipse(
        cx - r + o,
        cy - r - o * 0.4,
        cx + r + o,
        cy + r - o * 0.4,
        Some(WHITE),
        None,
    );
}

fn sun_or_moon(d: &mut Canvas, night: bool, cx: f32, cy: f32, r: f32, w: f32) {
    if night {
        moon(d, cx, cy, r, w)
    } else {
        sun(d, cx, cy, r, w)
    }
}

/// Cloud whose bounding box is (x, y, x+s, y+0.6s).
fn cloud(d: &mut Canvas, x: f32, y: f32, s: f32, fill: u8, w: f32) {
    let h = s * 0.6;
    let parts = [
        (x + s * 0.05, y + h * 0.40, x + s * 0.45, y + h),        // left puff
        (x + s * 0.22, y + h * 0.05, x + s * 0.70, y + h * 0.85), // top puff
        (x + s * 0.50, y + h * 0.30, x + s * 0.95, y + h),        // right puff
    ];
    // outlines first, then fills, gives a single merged silhouette
    for p in parts {
        d.ellipse(p.0 - w, p.1 - w, p.2 + w, p.3 + w, Some(BLACK), None);
    }
    d.rect(x + s * 0.25 - w, y + h * 0.6, x + s * 0.75 + w, y + h + w, BLACK);
    for p in parts {
        d.ellipse(p.0, p.1, p.2, p.3, Some(fill), None);
    }
    d.rect(x + s * 0.25, y + h * 0.6, x + s * 0.75, y + h - 0.5, fill);
}

/// Draws the icon for WMO weather `code` in the square (x, y, size).
pub fn draw_icon(d: &mut Canvas, code: i64, x: i32, y: i32, size: i32, night: bool) {
    let (x, y, s) = (x as f32, y as f32, size as f32);
    let w = (size / 28).max(2) as f32;
    let (cx, cy) = (x + s / 2.0, y + s / 2.0);
    match code {
        0 | 1 => return sun_or_moon(d, night, cx, cy, s * 0.24, w),
        2 => {
            sun_or_moon(d, night, x + s * 0.62, y + s * 0.32, s * 0.16, w);
            return cloud(d, x + s * 0.02, y + s * 0.35, s * 0.8, WHITE, w);
        }
        3 => {
            cloud(d, x + s * 0.25, y + s * 0.15, s * 0.7, LIGHT, w);
            return cloud(d, x, y + s * 0.3, s * 0.85, WHITE, w);
        }
        45 | 48 => {
            for k in 0..5 {
                let yy = y + s * (0.25 + k as f32 * 0.13);
                let off = s * 0.08 * (k % 2) as f32;
                d.line(x + s * 0.1 + off, yy, x + s * 0.9 - off, yy, DARK, w + 1.0);
            }
            return;
        }
        _ => {}
    }

    // precipitation / storms: cloud on top, symbols below
    cloud(
        d,
        x + s * 0.05,
        y + s * 0.05,
        s * 0.9,
        if code >= 95 { LIGHT } else { WHITE },
        w,
    );
    let by = y + s * 0.65;
    if code >= 95 {
        let bolt = [
            (cx + s * 0.05, by - s * 0.05),
            (cx - s * 0.12, by + s * 0.17),
            (cx, by + s * 0.17),
            (cx - s * 0.08, by + s * 0.35),
            (cx + s * 0.15, by + s * 0.1),
            (cx + s * 0.03, by + s * 0.1),
        ];
        d.polygon(&bolt, BLACK);
    } else if matches!(code, 71 | 73 | 75 | 77 | 85 | 86) {
        for k in 0..3 {
            let (fx, fy, r) = (
                x + s * (0.28 + k as f32 * 0.22),
                by + s * (0.1 + 0.1 * (k % 2) as f32),
                s * 0.09,
            );
            for a in 0..3 {
                let ang = a as f32 * PI / 3.0;
                let (c, sn) = (ang.cos() * r, ang.sin() * r);
                d.line(fx - c, fy - sn, fx + c, fy + sn, BLACK, w);
            }
        }
    } else {
        // drizzle / rain / showers
        let drops = if matches!(code, 51 | 53 | 55 | 56 | 57) { 2 } else { 3 };
        for k in 0..drops {
            let fx = x + s * (0.3 + k as f32 * (0.4 / (drops - 1).max(1) as f32));
            d.line(fx, by + s * 0.02, fx - s * 0.07, by + s * 0.25, BLACK, w + 1.0);
        }
    }
}
