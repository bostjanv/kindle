//! Blocking HTTP GET helpers.

use std::time::Duration;

use anyhow::{Context, Result};

const USER_AGENT: &str = concat!("kindle-dash/", env!("CARGO_PKG_VERSION"));
const MAX_BODY: u64 = 64 << 20;

fn agent(timeout_secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_secs)))
        .user_agent(USER_AGENT)
        .build()
        .into()
}

pub fn get_bytes(url: &str, timeout_secs: u64) -> Result<Vec<u8>> {
    let mut resp = agent(timeout_secs)
        .get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    Ok(resp.body_mut().with_config().limit(MAX_BODY).read_to_vec()?)
}

pub fn get_json(url: &str, query: &[(&str, String)], timeout_secs: u64) -> Result<serde_json::Value> {
    let mut req = agent(timeout_secs).get(url);
    for (k, v) in query {
        req = req.query(k, v);
    }
    let mut resp = req.call().with_context(|| format!("GET {url}"))?;
    Ok(resp.body_mut().read_json()?)
}

/// Like [`get_json`], but a non-2xx status is `Ok(None)` instead of an error.
pub fn get_json_opt(url: &str, timeout_secs: u64) -> Result<Option<serde_json::Value>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_secs)))
        .user_agent(USER_AGENT)
        .http_status_as_error(false)
        .build()
        .into();
    let mut resp = agent.get(url).call().with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    Ok(resp.body_mut().read_json().ok())
}
