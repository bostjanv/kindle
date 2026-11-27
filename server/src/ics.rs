//! Minimal iCalendar (RFC 5545) reader: VEVENTs with RRULE/RDATE/EXDATE
//! recurrence (expanded with the `rrule` crate) and RECURRENCE-ID overrides.

use std::collections::HashSet;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// One occurrence of an event, in the display timezone.
#[derive(Clone, Debug)]
pub struct Occurrence {
    pub all_day: bool,
    pub start: DateTime<Tz>,
    pub end: DateTime<Tz>,
    pub summary: String,
}

impl Occurrence {
    /// Does it overlap `[from, to)`? A zero-length occurrence counts if it starts inside.
    pub fn overlaps(&self, from: DateTime<Tz>, to: DateTime<Tz>) -> bool {
        self.start < to && (self.end > from || self.start >= from)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Time {
    Date(NaiveDate),
    Floating(NaiveDateTime),
    Utc(NaiveDateTime),
    Zoned(NaiveDateTime, Tz),
}

impl Time {
    fn is_date(&self) -> bool {
        matches!(self, Time::Date(_))
    }

    /// The timezone the value is expressed in (`local` for dates and floating times).
    fn zone(&self, local: Tz) -> Tz {
        match self {
            Time::Utc(_) => Tz::UTC,
            Time::Zoned(_, tz) => *tz,
            _ => local,
        }
    }

    fn resolve(&self, local: Tz) -> DateTime<Tz> {
        let (naive, tz) = match self {
            Time::Date(d) => (d.and_time(NaiveTime::MIN), local),
            Time::Floating(n) => (*n, local),
            Time::Utc(n) => return Utc.from_utc_datetime(n).with_timezone(&Tz::UTC),
            Time::Zoned(n, tz) => (*n, *tz),
        };
        localize(naive, tz)
    }
}

/// Local time to an instant; a time skipped by DST moves forward an hour.
fn localize(naive: NaiveDateTime, tz: Tz) -> DateTime<Tz> {
    tz.from_local_datetime(&naive)
        .earliest()
        .or_else(|| tz.from_local_datetime(&(naive + Duration::hours(1))).earliest())
        .unwrap_or_else(|| Utc.from_utc_datetime(&naive).with_timezone(&tz))
}

/// IANA name, or one wrapped in a vendor prefix like `/mozilla.org/20050126_1/Europe/Ljubljana`.
fn parse_tzid(id: &str) -> Option<Tz> {
    let id = id.trim().trim_matches('"');
    if let Ok(tz) = id.parse() {
        return Some(tz);
    }
    let parts: Vec<&str> = id.split('/').collect();
    (1..parts.len()).find_map(|i| parts[i..].join("/").parse().ok())
}

struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, key: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn parse(line: &str) -> Option<Prop> {
        // name and params end at the first ':' outside double quotes
        let mut in_quotes = false;
        let colon = line.char_indices().find_map(|(i, c)| match c {
            '"' => {
                in_quotes = !in_quotes;
                None
            }
            ':' if !in_quotes => Some(i),
            _ => None,
        })?;
        let (head, value) = (&line[..colon], &line[colon + 1..]);
        let mut parts = head.split(';');
        let name = parts.next()?.trim().to_ascii_uppercase();
        let params = parts
            .filter_map(|p| p.split_once('='))
            .map(|(k, v)| (k.trim().to_ascii_uppercase(), v.trim_matches('"').to_string()))
            .collect();
        Some(Prop {
            name,
            params,
            value: value.to_string(),
        })
    }

    /// The value as a list of times (DTSTART, EXDATE, ...).
    fn times(&self) -> Result<Vec<Time>> {
        let tz = self.param("TZID").and_then(parse_tzid);
        let date = self.param("VALUE") == Some("DATE");
        self.value
            .split(',')
            .filter(|v| !v.trim().is_empty())
            .map(|v| parse_time(v.trim(), date, tz))
            .collect()
    }

    fn time(&self) -> Result<Time> {
        self.times()?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("empty {}", self.name))
    }
}

fn parse_time(v: &str, date: bool, tz: Option<Tz>) -> Result<Time> {
    if date || v.len() == 8 {
        return Ok(Time::Date(NaiveDate::parse_from_str(&v[..8.min(v.len())], "%Y%m%d")?));
    }
    let (v, utc) = match v.strip_suffix('Z') {
        Some(v) => (v, true),
        None => (v, false),
    };
    let n = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").with_context(|| format!("bad date-time {v:?}"))?;
    Ok(match (utc, tz) {
        (true, _) => Time::Utc(n),
        (false, Some(tz)) => Time::Zoned(n, tz),
        (false, None) => Time::Floating(n),
    })
}

/// RFC 5545 duration: `[+-]P[nW][nD][T[nH][nM][nS]]`.
fn parse_duration(v: &str) -> Result<Duration> {
    let (neg, rest) = match v.as_bytes().first() {
        Some(b'-') => (true, &v[1..]),
        Some(b'+') => (false, &v[1..]),
        _ => (false, v),
    };
    let rest = rest.strip_prefix('P').ok_or_else(|| anyhow!("bad duration {v:?}"))?;
    let (mut total, mut num) = (0i64, String::new());
    for c in rest.chars() {
        match c {
            '0'..='9' => num.push(c),
            'T' => {}
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let n: i64 = num.parse().with_context(|| format!("bad duration {v:?}"))?;
                num.clear();
                total += n * match c {
                    'W' => 7 * 86400,
                    'D' => 86400,
                    'H' => 3600,
                    'M' => 60,
                    _ => 1,
                };
            }
            _ => bail!("bad duration {v:?}"),
        }
    }
    Ok(Duration::seconds(if neg { -total } else { total }))
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push(' '),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Default)]
pub struct Event {
    uid: String,
    summary: Option<String>,
    start: Option<Time>,
    end: Option<Time>,
    duration: Option<Duration>,
    rrules: Vec<String>,
    rdates: Vec<Time>,
    exdates: Vec<Time>,
    recurrence_id: Option<Time>,
}

pub struct Calendar {
    events: Vec<Event>,
}

impl Calendar {
    pub fn parse(data: &[u8]) -> Result<Calendar> {
        let text = String::from_utf8_lossy(data);
        // unfold: a line starting with a space or tab continues the previous one
        let mut lines: Vec<String> = Vec::new();
        for raw in text.split('\n') {
            let raw = raw.strip_suffix('\r').unwrap_or(raw);
            match raw.chars().next() {
                Some(' ' | '\t') if !lines.is_empty() => lines.last_mut().unwrap().push_str(&raw[1..]),
                _ => lines.push(raw.to_string()),
            }
        }
        if !lines.iter().any(|l| l.trim().eq_ignore_ascii_case("BEGIN:VCALENDAR")) {
            bail!("not an iCalendar file");
        }

        let mut events = Vec::new();
        let mut current: Option<Event> = None;
        let mut depth = 0; // nesting inside the VEVENT (VALARM etc.)
        for line in &lines {
            let Some(p) = Prop::parse(line) else { continue };
            let value = p.value.trim().to_ascii_uppercase();
            match (p.name.as_str(), current.is_some()) {
                ("BEGIN", false) if value == "VEVENT" => current = Some(Event::default()),
                ("BEGIN", true) => depth += 1,
                ("END", true) if depth > 0 => depth -= 1,
                ("END", true) if value == "VEVENT" => events.push(current.take().unwrap()),
                (_, true) if depth == 0 => {
                    let ev = current.as_mut().unwrap();
                    if let Err(e) = ev.set(&p) {
                        log::debug!("ics: skipping {}: {e:#}", p.name);
                    }
                }
                _ => {}
            }
        }
        Ok(Calendar { events })
    }

    /// Occurrences overlapping `[from, to)`, in timezone `tz`; floating times and dates are taken as `tz`.
    pub fn between(&self, from: DateTime<Tz>, to: DateTime<Tz>, tz: Tz) -> Vec<Occurrence> {
        // instances replaced by a RECURRENCE-ID override: (uid, original start)
        let overridden: HashSet<(&str, i64)> = self
            .events
            .iter()
            .filter_map(|e| Some((e.uid.as_str(), e.recurrence_id.as_ref()?.resolve(tz).timestamp())))
            .collect();
        let mut out = Vec::new();
        for ev in &self.events {
            if let Err(e) = ev.expand(from, to, tz, &overridden, &mut out) {
                log::warn!("ics: event {:?}: {e:#}", ev.summary.as_deref().unwrap_or(&ev.uid));
            }
        }
        out
    }
}

impl Event {
    fn set(&mut self, p: &Prop) -> Result<()> {
        match p.name.as_str() {
            "UID" => self.uid = p.value.clone(),
            "SUMMARY" => self.summary = Some(unescape(&p.value)),
            "DTSTART" => self.start = Some(p.time()?),
            "DTEND" => self.end = Some(p.time()?),
            "DURATION" => self.duration = Some(parse_duration(p.value.trim())?),
            "RRULE" => self.rrules.push(p.value.trim().to_string()),
            "RDATE" if p.param("VALUE") != Some("PERIOD") => self.rdates.extend(p.times()?),
            "EXDATE" => self.exdates.extend(p.times()?),
            "RECURRENCE-ID" => self.recurrence_id = Some(p.time()?),
            _ => {}
        }
        Ok(())
    }

    fn expand(
        &self,
        from: DateTime<Tz>,
        to: DateTime<Tz>,
        tz: Tz,
        overridden: &HashSet<(&str, i64)>,
        out: &mut Vec<Occurrence>,
    ) -> Result<()> {
        let Some(start) = &self.start else { return Ok(()) };
        let all_day = start.is_date();
        let first = start.resolve(tz);
        let length = match (&self.end, self.duration) {
            (Some(end), _) => end.resolve(tz) - first,
            (None, Some(d)) => d,
            (None, None) if all_day => Duration::days(1),
            _ => Duration::zero(),
        }
        .max(Duration::zero());
        let summary = self.summary.clone().unwrap_or_else(|| "(no title)".into());
        let mut push = |s: DateTime<Tz>| {
            let occ = Occurrence {
                all_day,
                start: s.with_timezone(&tz),
                end: (s + length).with_timezone(&tz),
                summary: summary.clone(),
            };
            if occ.overlaps(from, to) {
                out.push(occ);
            }
        };

        if self.recurrence_id.is_some() || (self.rrules.is_empty() && self.rdates.is_empty()) {
            push(first);
            return Ok(());
        }

        // Expand in the event's own zone, so DST shifts keep the wall-clock time.
        let zone = start.zone(tz);
        let rz = rrule::Tz::Tz(zone);
        let dtstart = first.with_timezone(&rz);
        let mut set = rrule::RRuleSet::new(dtstart);
        for r in &self.rrules {
            set = set.rrule(build_rrule(r, start, zone, dtstart)?);
        }
        for t in &self.rdates {
            set = set.rdate(t.resolve(zone).with_timezone(&rz));
        }
        for t in &self.exdates {
            set = set.exdate(t.resolve(zone).with_timezone(&rz));
        }
        let earliest = from - length;
        for (n, dt) in (&set).into_iter().enumerate() {
            let dt = dt.with_timezone(&zone);
            if dt >= to || n > 200_000 {
                break;
            }
            if dt < earliest || overridden.contains(&(self.uid.as_str(), dt.timestamp())) {
                continue;
            }
            push(dt);
        }
        Ok(())
    }
}

/// Parses an RRULE value. UNTIL is converted to UTC first: calendars often give it
/// as a date or local time, which the `rrule` crate rejects for a zoned DTSTART.
fn build_rrule(
    value: &str,
    start: &Time,
    zone: Tz,
    dtstart: DateTime<rrule::Tz>,
) -> Result<rrule::RRule<rrule::Validated>> {
    let mut until = None;
    let parts: Vec<&str> = value
        .split(';')
        .filter(|part| match part.split_once('=') {
            Some((k, v)) if k.eq_ignore_ascii_case("UNTIL") => {
                until = Some(v.to_string());
                false
            }
            _ => !part.is_empty(),
        })
        .collect();
    let mut rule = rrule::RRule::from_str(&parts.join(";")).map_err(|e| anyhow!("RRULE {value:?}: {e}"))?;
    if let Some(u) = until {
        let t = parse_time(&u, false, None)?;
        let u = match t {
            // a date UNTIL includes that whole day
            Time::Date(d) if !start.is_date() => localize(d.and_hms_opt(23, 59, 59).unwrap(), zone),
            t => t.resolve(zone),
        };
        let u = u
            .with_timezone(&rrule::Tz::UTC)
            .max(dtstart.with_timezone(&rrule::Tz::UTC));
        rule = rule.until(u);
    }
    rule.validate(dtstart).map_err(|e| anyhow!("RRULE {value:?}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART;TZID=Europe/Ljubljana:20261101T183000\r\n\
DTEND;TZID=Europe/Ljubljana:20261101T193000\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,TH\r\n\
EXDATE;TZID=Europe/Ljubljana:20261203T183000\r\nSUMMARY:Yoga\\, weekly\r\n long\r\nBEGIN:VALARM\r\n\
SUMMARY:alarm\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a\r\n\
RECURRENCE-ID;TZID=Europe/Ljubljana:20261130T183000\r\nDTSTART;TZID=Europe/Ljubljana:20261130T200000\r\n\
DTEND;TZID=Europe/Ljubljana:20261130T210000\r\nSUMMARY:Moved\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:b\r\nDTSTART;VALUE=DATE:20250102\r\nRRULE:FREQ=YEARLY\r\nSUMMARY:Birthday\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:c\r\nDTSTART;TZID=Europe/Ljubljana:20261020T090000\r\nRRULE:FREQ=DAILY;UNTIL=20261129\r\n\
SUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn expands_recurrences_and_overrides() {
        let tz: Tz = "Europe/Ljubljana".parse().unwrap();
        let cal = Calendar::parse(ICS.as_bytes()).unwrap();
        let from = tz.with_ymd_and_hms(2026, 11, 27, 0, 0, 0).unwrap();
        let mut occ = cal.between(from, from + Duration::days(14), tz);
        occ.sort_by_key(|o| o.start);
        let got: Vec<String> = occ
            .iter()
            .map(|o| format!("{} {}", o.start.format("%m-%d %H:%M"), o.summary))
            .collect();
        assert_eq!(
            got,
            [
                "11-27 09:00 Standup",
                "11-28 09:00 Standup",
                "11-29 09:00 Standup",
                "11-30 20:00 Moved",
                "12-07 18:30 Yoga, weeklylong",
                "12-10 18:30 Yoga, weeklylong",
            ]
        );
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("PT1H30M").unwrap(), Duration::minutes(90));
        assert_eq!(parse_duration("P1W").unwrap(), Duration::days(7));
        assert_eq!(parse_duration("-P1DT1S").unwrap(), -Duration::seconds(86401));
    }

    #[test]
    fn vendor_tzid() {
        assert_eq!(
            parse_tzid("/mozilla.org/20050126_1/Europe/Ljubljana"),
            Some(chrono_tz::Europe::Ljubljana)
        );
    }
}
