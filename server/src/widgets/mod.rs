//! Widget registry.
//!
//! A widget is built from its config section (a TOML table) and draws itself
//! into a [`Bounds`] on the canvas. To add one (e.g. transit), create a module
//! here, add it to [`create`] and put `widget = "transit"` in a `[[screens.layout]]`.

use anyhow::{Result, bail};
use chrono::format::{Item, StrftimeItems};
use toml::Table;

use crate::canvas::{Bounds, Canvas};
use crate::render::Context;

mod calendar;
mod clock;
mod status;
mod weather;
mod xkcd;

pub trait Widget: Send {
    /// Shown in the error message when drawing fails.
    fn name(&self) -> &'static str;
    fn draw(&mut self, c: &mut Canvas, b: Bounds, ctx: &Context) -> Result<()>;
}

pub const NAMES: [&str; 5] = ["calendar", "clock", "status", "weather", "xkcd"];

pub fn create(name: &str, cfg: &Table) -> Result<Box<dyn Widget>> {
    Ok(match name {
        "calendar" => Box::new(calendar::Calendar::new(cfg)?),
        "clock" => Box::new(clock::Clock::new(cfg)?),
        "status" => Box::new(status::Status::new(cfg)),
        "weather" => Box::new(weather::Weather::new(cfg)?),
        "xkcd" => Box::new(xkcd::Xkcd::new(cfg)),
        _ => bail!("unknown widget {name:?}; available: {}", NAMES.join(", ")),
    })
}

/// Rejects a `strftime` format chrono cannot render (formatting it would panic).
pub fn check_format(fmt: &str) -> Result<String> {
    if StrftimeItems::new(fmt).any(|i| matches!(i, Item::Error)) {
        bail!("invalid time format {fmt:?}");
    }
    Ok(fmt.to_string())
}
