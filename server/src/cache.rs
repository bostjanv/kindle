//! Keeps the last good result of an expensive fetch; serves stale data on failure.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub struct Cached<T> {
    ttl: f64,
    value: Option<T>,
    stamp: f64,
    /// Set while the last refresh failed and `value` is stale.
    pub error: Option<String>,
}

impl<T> Cached<T> {
    pub fn new(ttl_seconds: f64) -> Self {
        Cached {
            ttl: ttl_seconds,
            value: None,
            stamp: 0.0,
            error: None,
        }
    }

    pub fn get(&mut self, fetch: impl FnOnce() -> Result<T>) -> Result<&T> {
        if self.value.is_none() || now() - self.stamp > self.ttl {
            match fetch() {
                Ok(v) => {
                    self.value = Some(v);
                    self.stamp = now();
                    self.error = None;
                }
                Err(e) => {
                    log::warn!("fetch failed: {e:#}");
                    self.error = Some(format!("{e:#}"));
                    self.stamp = now() - self.ttl + 60.0; // retry in a minute, not every render
                    if self.value.is_none() {
                        return Err(e);
                    }
                }
            }
        }
        Ok(self.value.as_ref().expect("value set above"))
    }
}
