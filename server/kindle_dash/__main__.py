"""kindle-dash: serve (or render once) the Kindle dashboard PNG.

  kindle-dash serve  [--config config.toml]          # HTTP server for the Kindle
  kindle-dash render [--config config.toml] -o out.png
"""
from __future__ import annotations

import argparse
import logging
import threading
import time
import tomllib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

from .render import Dashboard

log = logging.getLogger("kindle_dash")

PREVIEW = b"""<!doctype html><meta charset=utf-8><title>Kindle dashboard preview</title>
<style>body{background:#888;display:grid;place-items:center;min-height:100vh;margin:0}
img{box-shadow:0 0 0 18px #222,0 8px 40px #000;image-rendering:pixelated}</style>
<img id=i src=dashboard.png><script>setInterval(()=>i.src='dashboard.png?t='+Date.now(),30000)</script>"""


def load_config(path: str) -> dict:
    with open(path, "rb") as f:
        return tomllib.load(f)


def make_handler(dash: Dashboard, lock: threading.Lock):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            url = urlparse(self.path)
            query = {k: v[-1] for k, v in parse_qs(url.query).items()}
            if url.path in ("/", "/preview"):
                return self._send(200, "text/html; charset=utf-8", PREVIEW)
            if url.path == "/health":
                return self._send(200, "text/plain", b"ok\n")
            if url.path == "/time":  # Unix time; the Kindle times its refreshes by it
                return self._send(200, "text/plain", f"{time.time():.0f}\n".encode())
            if url.path in ("/dashboard.png", "/kindle.png"):
                try:
                    with lock:  # widgets share caches; render one at a time
                        body = dash.render_png(query)
                except Exception as e:
                    log.exception("render failed")
                    return self._send(500, "text/plain", f"render failed: {e}\n".encode())
                return self._send(200, "image/png", body)
            self._send(404, "text/plain", b"not found\n")

        def _send(self, code, ctype, body):
            self.send_response(code)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, fmt, *args):
            log.info("%s %s", self.address_string(), fmt % args)

    return Handler


def main():
    ap = argparse.ArgumentParser(prog="kindle-dash")
    ap.add_argument("command", choices=["serve", "render"])
    ap.add_argument("--config", "-c", default="config.toml")
    ap.add_argument("--output", "-o", default="dashboard.png")
    ap.add_argument("--host", help="override [server].host")
    ap.add_argument("--port", type=int, help="override [server].port")
    ap.add_argument("--battery", help="(render) battery %% to show in the footer")
    ap.add_argument("-v", "--verbose", action="store_true")
    a = ap.parse_args()
    logging.basicConfig(level=logging.DEBUG if a.verbose else logging.INFO, format="%(asctime)s %(levelname)s %(message)s")

    cfg = load_config(a.config)
    dash = Dashboard(cfg)
    if a.command == "render":
        Path(a.output).write_bytes(dash.render_png({"battery": a.battery} if a.battery else {}))
        print(f"wrote {a.output}")
        return

    host = a.host or cfg.get("server", {}).get("host", "0.0.0.0")
    port = a.port or int(cfg.get("server", {}).get("port", 8080))
    srv = ThreadingHTTPServer((host, port), make_handler(dash, threading.Lock()))
    log.info("serving on http://%s:%d/dashboard.png (preview at /)", host, port)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
