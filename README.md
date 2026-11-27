# Kindle dashboard

Turns a **Kindle Keyboard 3G (EU, serial `B00A…`, FW 3.2.1)** into an always-on
e-ink dashboard with several screens (clock + weather + calendar, a full calendar,
xkcd). The **page-turn buttons** switch screens.

```
 PC on the LAN                               Kindle (jailbroken)
 ┌──────────────────────────┐   HTTP GET    ┌──────────────────────────────┐
 │ kindle-dash serve        │ ◄──────────── │ dashboard.sh (every 60 s)    │
 │  clock · weather · cal   │  600x800 PNG  │  wget → eips -g dashboard.png│
 │  → /dashboard.png        │ ────────────► │  Launchpad hotkeys: Shift D …│
 └──────────────────────────┘               └──────────────────────────────┘
```

The PC renders the picture and the Kindle only downloads and displays it, so new
widgets (xkcd, transit and so on) only need server changes on the PC.

## Layout

| Path | What |
|---|---|
| `server/` | `kindle-dash` Rust server: renders the PNG, widgets in `src/widgets/` |
| `kindle/dashboard/` | goes to the Kindle drive as `dashboard/`: refresh loop + `dashboard.conf` (server IP) |
| `kindle/launchpad/dashboard.ini` | Launchpad hotkeys |
| `kindle/package/` | autostart OTA package (built with KindleTool, signed with the jailbreak key) |
| `tools/usbdisk/` | userspace USB mass-storage → FUSE driver (for hosts without usb-storage/vfat) |
| `tools/deploy.sh` | copies each stage onto the Kindle |
| `downloads/` | jailbreak 0.13.N, USBNetwork 0.57.N, Launchpad 0.0.1d from MobileRead (not committed) |

## 1. Run the server on the PC

Needs a Rust toolchain ([rustup](https://rustup.rs)). Fonts are built into the binary.

```sh
cd server
cargo build --release
cp config.example.toml config.toml     # set location, timezone, calendar sources
target/release/kindle-dash serve -c config.toml
target/release/kindle-dash render -c config.toml -o out.png --screen 1   # one PNG, no server
```

Open `http://<pc-ip>:8080/` in a browser to preview the dashboard. The Kindle
fetches `http://<pc-ip>:8080/dashboard.png`. For Google Calendar, use
*Settings → your calendar → Secret address in iCal format*.
For a [Remind](https://dianne.skoll.ca/projects/remind/) file (or a directory of
`*.rem` files), add `{ remind = "~/.reminders" }` to `sources`. It is read with
`remind -ppp`, so the `remind` program must be on PATH (`apt install remind`).

To keep it running, use `server/kindle-dash.service` (a systemd user unit).

**Running the server on a Raspberry Pi (64-bit OS):** cross-compile on the PC.
One-time setup: `sudo apt install gcc-aarch64-linux-gnu` and
`rustup target add aarch64-unknown-linux-musl`. Then, in `server/`:

```sh
cargo build-aarch64          # → target/aarch64-unknown-linux-musl/release/kindle-dash
scp target/aarch64-unknown-linux-musl/release/kindle-dash config.toml pi@<pi-ip>:kindle-dash/
```

The binary is static, so it runs on any aarch64 Linux. `cargo build-aarch64-gnu` builds a
glibc binary instead, which only runs where glibc is at least as new as on the build host.
The aliases and linker settings are in `server/.cargo/config.toml`.

## 2. Jailbreak + install (one-time)

`tools/quickcopy.sh <stage>…` waits for the Kindle, copies, verifies by reading back, then ejects.
Autostart: `tools/quickcopy.sh autostart` (package + `dashboard/AUTOSTART` flag), then Update Your Kindle.

Every step works the same way: plug the Kindle in so it shows **USB drive mode**,
copy a file to it, unplug, then on the Kindle press
**Home → Menu → Settings → Menu → Update Your Kindle**.

If the PC mounts the Kindle as a normal drive, copy the files by hand.
Otherwise `tools/deploy.sh` does it over raw USB:

| Step | Copy to the Kindle | Then on the Kindle |
|---|---|---|
| 1. Jailbreak | `tools/deploy.sh jailbreak` → `Update_jailbreak_0.13.N_k3gb_install.bin` (FW 3.2.1+; the `-3.0-to-3.2` file is only for FW ≤ 3.2; on 3.2.1 it fails with U004) | Update Your Kindle; it reboots |
| 2. Launchpad | `tools/deploy.sh launchpad` → `update_launchpad_0.0.1d_k3gb_install.bin` | Update Your Kindle |
| 3. Dashboard | `SERVER_IP=<pc-ip> tools/quickcopy.sh launchpad dashboard` (or `tools/deploy.sh dashboard`) → `dashboard/`, `launchpad/dashboard.ini`, `Update_dashboard_k3gb_install.bin` | Update Your Kindle (installs the autostart hook) |

Use only the `k3gb` packages. They are the builds for the EU 3G model (`B00A`).

## 3. Use it

**Next Page / Prev Page** (either side) or the 5-way **left/right** switch between the
screens defined under `[[screens]]` in `config.toml`. The dots in the footer show which
one is on.

| Hotkey (press Shift, release, then the letters) | Action |
|---|---|
| `Shift D D` | start the dashboard |
| `Shift D X` | stop it and bring back the normal Kindle UI. **Do this before plugging into USB.** |
| `Shift D R` | redraw now with a full refresh |
| `Shift D A` | toggle autostart at boot |
| `Shift Space` | make Launchpad reload its hotkeys (e.g. after editing an `.ini`) |

**Changing the server IP:** edit `dashboard/dashboard.conf` on the Kindle drive
(`SERVER_IP=`, `SERVER_PORT=`, or `SERVER_URL=`), then restart with `Shift D X`, `Shift D D`.

**Power:** a 60 s refresh keeps Wi-Fi busy, so keep the Kindle plugged into a USB
charger. On battery, set `INTERVAL=300` or more. The clock then shows the time of
the last refresh.

## Adding a widget

Create `server/src/widgets/transit.rs`:

```rust
use anyhow::Result;
use toml::Table;

use super::Widget;
use crate::canvas::{Bounds, Canvas};
use crate::render::Context;

pub struct Transit { /* settings, Cached<...> data */ }

impl Transit {
    pub fn new(cfg: &Table) -> Result<Transit> { ... }   // cfg: the [transit] section
}

impl Widget for Transit {
    fn name(&self) -> &'static str { "Transit" }
    fn draw(&mut self, c: &mut Canvas, b: Bounds, ctx: &Context) -> Result<()> { ... }  // draw inside b
}
```

Add `mod transit;` and a `"transit"` arm to `create()` in `src/widgets/mod.rs`, then add
`[[screens.layout]] widget = "transit"` (and a `[transit]` section) to `config.toml`.
Wrap network calls in `cache::Cached` so a failure shows the last good data instead
of a blank panel; an error returned from `draw` is shown in the widget's box.

## Undo

Run `Update_dashboard_k3gb_uninstall.bin` (in `kindle/package/out/`) and the
`*_uninstall.bin` packages from `downloads/x/` the same way, jailbreak last.
