//! Events from Remind files, via `remind -ppp` (JSON calendar output).

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::Value;

use crate::ics::Occurrence;

/// Runs `remind` on `path` (a file or a directory of `*.rem` files) for `months`
/// months from `start`. Times are taken to be in `zone`; results are in `tz`.
pub fn load(path: &Path, start: NaiveDate, months: u32, zone: Tz, tz: Tz) -> Result<Vec<Occurrence>> {
    // remind prints an empty calendar for a missing path, so report it instead
    if !path.exists() {
        bail!("{} not found", path.display());
    }
    let out = Command::new("remind")
        .arg(format!("-pppq{months}"))
        .arg("-b2")
        .arg(path)
        .arg(start.format("%Y-%m-%d").to_string())
        .output()
        .context("running remind (is it installed? apt install remind)")?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
        bail!(
            "remind failed: {}",
            if last.is_empty() {
                out.status.to_string()
            } else {
                last.to_string()
            }
        );
    }
    let months: Value = serde_json::from_slice(&out.stdout).context("parsing remind output (remind too old?)")?;
    let mut occ = Vec::new();
    let mut seen = HashSet::new();
    for month in months.as_array().into_iter().flatten() {
        for e in month["entries"].as_array().into_iter().flatten() {
            // an event crossing midnight is listed again on the next day, with the same start
            let start = e.get("eventstart").or_else(|| e.get("date"));
            if !seen.insert((
                e["filename"].to_string(),
                e["lineno"].to_string(),
                start.map(Value::to_string),
            )) {
                continue;
            }
            if let Some(o) = entry(e, zone, tz) {
                occ.push(o);
            }
        }
    }
    Ok(occ)
}

fn entry(e: &Value, zone: Tz, tz: Tz) -> Option<Occurrence> {
    let text = |k: &str| e[k].as_str();
    let summary = match text("passthru") {
        None => match text("calendar_body") {
            Some("") => return None, // %"%": no calendar entry
            Some(b) => b,
            None => text("body")?,
        },
        // colored reminders are ordinary ones; other SPECIALs (SHADE, MOON, ...) are not events
        Some("COLOR" | "COLOUR") => text("plain_body").or_else(|| text("body").map(strip_color))?,
        Some(_) => return None,
    };
    let summary = summary.trim().replace("[\"[\"]", "[");

    if let Some(start) = text("eventstart") {
        let naive = NaiveDateTime::parse_from_str(start, "%Y-%m-%dT%H:%M").ok()?;
        let utc = text("tags").is_some_and(|t| t.split(',').any(|t| t == "rem2ics_utc"));
        let s = if utc {
            Utc.from_utc_datetime(&naive).with_timezone(&tz)
        } else {
            zone.from_local_datetime(&naive).earliest()?.with_timezone(&tz)
        };
        let minutes = e["eventduration"].as_i64().unwrap_or(0);
        return Some(Occurrence {
            all_day: false,
            start: s,
            end: s + Duration::minutes(minutes),
            summary,
        });
    }
    let day = NaiveDate::parse_from_str(text("date")?, "%Y-%m-%d").ok()?;
    let midnight = |d: NaiveDate| tz.from_local_datetime(&d.and_time(NaiveTime::MIN)).earliest();
    Some(Occurrence {
        all_day: true,
        start: midnight(day)?,
        end: midnight(day.succ_opt()?)?,
        summary,
    })
}

/// "255 0 0 Text" -> "Text"
fn strip_color(body: &str) -> &str {
    body.splitn(4, ' ').nth(3).unwrap_or(body)
}
