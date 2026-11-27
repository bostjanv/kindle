//! kindle-dash: serve (or render once) the Kindle dashboard PNG.
//!
//!   kindle-dash serve  [--config config.toml]          # HTTP server for the Kindle
//!   kindle-dash render [--config config.toml] -o out.png

mod cache;
mod canvas;
mod config;
mod http;
mod icons;
mod ics;
mod remind;
mod render;
mod widgets;

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use tiny_http::{Header, Request, Response, Server};

use render::Dashboard;

const PREVIEW: &str = r#"<!doctype html><meta charset=utf-8><title>Kindle dashboard preview</title>
<style>body{background:#888;display:grid;place-items:center;min-height:100vh;margin:0}
img{box-shadow:0 0 0 18px #222,0 8px 40px #000;image-rendering:pixelated}</style>
<img id=i src=dashboard.png><script>setInterval(()=>i.src='dashboard.png?t='+Date.now(),30000)</script>"#;

#[derive(Clone, Copy, ValueEnum)]
enum Command {
    /// HTTP server for the Kindle
    Serve,
    /// Render one PNG to --output
    Render,
}

#[derive(Parser)]
#[command(
    name = "kindle-dash",
    version,
    about = "Renders an e-ink dashboard PNG for a jailbroken Kindle"
)]
struct Args {
    command: Command,
    #[arg(short, long, default_value = "config.toml")]
    config: String,
    #[arg(short, long, default_value = "dashboard.png")]
    output: String,
    /// Override [server].host
    #[arg(long)]
    host: Option<String>,
    /// Override [server].port
    #[arg(long)]
    port: Option<u16>,
    /// (render) battery % to show in the footer
    #[arg(long)]
    battery: Option<String>,
    /// (render) screen to draw
    #[arg(long)]
    screen: Option<i64>,
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let a = Args::parse();
    env_logger::Builder::new()
        .filter_level(if a.verbose {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .parse_default_env()
        .format(|buf, r| {
            writeln!(
                buf,
                "{} {} {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                r.level(),
                r.args()
            )
        })
        .init();

    let text = std::fs::read_to_string(&a.config).with_context(|| format!("reading {}", a.config))?;
    let cfg: toml::Table = toml::from_str(&text).with_context(|| format!("parsing {}", a.config))?;
    let mut dash = Dashboard::new(&cfg)?;

    if let Command::Render = a.command {
        let mut query = HashMap::new();
        if let Some(b) = a.battery {
            query.insert("battery".into(), b);
        }
        if let Some(s) = a.screen {
            query.insert("screen".into(), s.to_string());
        }
        std::fs::write(&a.output, dash.render_png(&query)?).with_context(|| format!("writing {}", a.output))?;
        println!("wrote {}", a.output);
        return Ok(());
    }

    let server_cfg = config::table(&cfg, "server").cloned().unwrap_or_default();
    let host = a.host.unwrap_or_else(|| config::str_or(&server_cfg, "host", "0.0.0.0"));
    let port = a.port.unwrap_or(config::i64_or(&server_cfg, "port", 8080) as u16);
    let server =
        Arc::new(Server::http((host.as_str(), port)).map_err(|e| anyhow::anyhow!("listening on {host}:{port}: {e}"))?);
    log::info!("serving on http://{host}:{port}/dashboard.png (preview at /)");

    // widgets share caches, so render one at a time; other requests stay responsive
    let dash = Arc::new(Mutex::new(dash));
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let (server, dash) = (server.clone(), dash.clone());
            std::thread::spawn(move || {
                while let Ok(req) = server.recv() {
                    handle(req, &dash);
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter(|kv| !kv.is_empty())
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((decode(k), decode(v)))
        })
        .collect()
}

fn handle(req: Request, dash: &Mutex<Dashboard>) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let (code, ctype, body): (u16, &str, Vec<u8>) = match path {
        "/" | "/preview" => (200, "text/html; charset=utf-8", PREVIEW.into()),
        "/health" => (200, "text/plain", b"ok\n".to_vec()),
        // Unix time; the Kindle times its refreshes by it
        "/time" => {
            let t = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            (200, "text/plain", format!("{:.0}\n", t).into_bytes())
        }
        "/dashboard.png" | "/kindle.png" => {
            let query = parse_query(query);
            let result = match dash.lock() {
                Ok(mut d) => d.render_png(&query),
                Err(poisoned) => poisoned.into_inner().render_png(&query),
            };
            match result {
                Ok(png) => (200, "image/png", png),
                Err(e) => {
                    log::error!("render failed: {e:#}");
                    (500, "text/plain", format!("render failed: {e:#}\n").into_bytes())
                }
            }
        }
        _ => (404, "text/plain", b"not found\n".to_vec()),
    };
    let addr = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    log::info!("{addr} \"{} {url}\" {code}", req.method());
    let resp = Response::from_data(body)
        .with_status_code(code)
        .with_header(Header::from_bytes("Content-Type", ctype).unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
    if let Err(e) = req.respond(resp) {
        log::debug!("{addr}: response not sent: {e}");
    }
}
