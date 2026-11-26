#!/bin/bash
# Copy files to the Kindle's USB drive without kernel usb-storage/vfat support,
# using tools/usbdisk (userspace USB mass storage -> FUSE) + mtools.
#
#   tools/deploy.sh jailbreak   # step 1: jailbreak .bin to the drive root
#   tools/deploy.sh launchpad   # step 2: Launchpad hotkey manager .bin
#   tools/deploy.sh dashboard   # step 3: dashboard scripts, hotkeys, autostart .bin
#   tools/deploy.sh ls [dir]    # list a directory on the Kindle
#   tools/deploy.sh get <path> <local>   # copy a file off the Kindle
#   tools/deploy.sh shell       # mount and drop into a shell with mtools ready (drive k:)
#
# If your machine can mount the Kindle normally, you can instead just copy the
# same files by hand (see README.md).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEV=${KINDLE_DEV:-k3gb}
MNT=${USBDISK_MNT:-/tmp/kindle-usbdisk}
USBDISK=$ROOT/tools/usbdisk/usbdisk
DL=$ROOT/downloads/x

[ "$USBDISK" -nt "$ROOT/tools/usbdisk/usbdisk.c" ] || gcc -O2 -o "$USBDISK" "$ROOT/tools/usbdisk/usbdisk.c" $(pkg-config --cflags --libs libusb-1.0 fuse3)

mkdir -p "$MNT"
cleanup() { sync; sudo fusermount3 -u "$MNT" 2>/dev/null || true; }
trap cleanup EXIT
# Always mount afresh: a mount left over from an earlier run holds a USB handle
# that is dead once the Kindle was replugged or ejected, and only gives I/O errors.
sudo fusermount3 -u "$MNT" 2>/dev/null || true
sudo "$USBDISK" -s -o allow_other "$MNT" || exit 1
for _ in $(seq 20); do [ -f "$MNT/disk" ] && break; sleep 0.5; done
[ -f "$MNT/disk" ] || { echo "Kindle drive not available (is it plugged in and in USB drive mode?)" >&2; exit 1; }

# Superfloppy (FAT directly on the device) or partitioned?
export MTOOLSRC=$(mktemp)
if [ "$(od -An -tx1 -j510 -N2 "$MNT/disk" | tr -d ' ')" = "55aa" ] && \
   ! od -An -c -j54 -N5 "$MNT/disk" | tr -d ' ' | grep -q FAT && \
   ! od -An -c -j82 -N5 "$MNT/disk" | tr -d ' ' | grep -q FAT; then
    echo "drive k: file=\"$MNT/disk\" partition=1" > "$MTOOLSRC"
else
    echo "drive k: file=\"$MNT/disk\"" > "$MTOOLSRC"
fi
echo "mtools_skip_check=1" >> "$MTOOLSRC"
minfo k: | grep -E "^(disk type|volume label)" || true

put() { echo "  -> k:/$2"; mcopy -o -Q "$1" "k:/$2"; }
mkd() { mmd "k:/$1" 2>/dev/null || true; }

case "${1:-}" in
    jailbreak)
        put "$DL/kindle-jailbreak-0.13.N/Update_jailbreak_0.13.N_${DEV}_install.bin" \
            "Update_jailbreak_0.13.N_${DEV}_install.bin"
        ;;
    launchpad)
        put "$DL/lpad-pkg-001d/update_launchpad_0.0.1d_${DEV}_install.bin" "update_launchpad_0.0.1d_${DEV}_install.bin"
        ;;
    dashboard)
        mkd dashboard
        put "$ROOT/kindle/dashboard/dashboard.sh" dashboard/dashboard.sh
        if mdir "k:/dashboard/dashboard.conf" >/dev/null 2>&1 && [ -z "${OVERWRITE_CONF:-}" ]; then
            echo "  (keeping existing dashboard/dashboard.conf; set OVERWRITE_CONF=1 to replace)"
        else
            tmp=$(mktemp)
            sed "s/^SERVER_IP=.*/SERVER_IP=${SERVER_IP:-192.168.1.100}/; s/^SERVER_PORT=.*/SERVER_PORT=${SERVER_PORT:-8080}/" \
                "$ROOT/kindle/dashboard/dashboard.conf" > "$tmp"
            put "$tmp" dashboard/dashboard.conf
            rm -f "$tmp"
        fi
        [ -n "${AUTOSTART:-}" ] && { : > /tmp/AUTOSTART.$$; put /tmp/AUTOSTART.$$ dashboard/AUTOSTART; rm -f /tmp/AUTOSTART.$$; }
        if mdir k:/launchpad >/dev/null 2>&1; then
            put "$ROOT/kindle/launchpad/dashboard.ini" launchpad/dashboard.ini
        else
            echo "  !! k:/launchpad not found: install Launchpad first (deploy.sh launchpad), then rerun" >&2
        fi
        [ -f "$ROOT/kindle/package/out/Update_dashboard_${DEV}_install.bin" ] || \
            KINDLETOOL=${KINDLETOOL:-$ROOT/build/KindleTool/KindleTool/Release/kindletool} "$ROOT/kindle/package/build.sh" "$DEV" >/dev/null
        put "$ROOT/kindle/package/out/Update_dashboard_${DEV}_install.bin" "Update_dashboard_${DEV}_install.bin"
        ;;
    ls) mdir -a "k:/${2:-}" ;;
    tree) mdir -a -/ "k:/${2:-}" ;;
    get) mcopy -o "k:/$2" "$3" ;;
    shell) echo "mtools ready: try 'mdir k:'; exit to unmount"; MTOOLSRC=$MTOOLSRC bash -i ;;
    *) sed -n '2,13p' "$0"; exit 1 ;;
esac
sync
echo "done. Unplug the Kindle (or eject it), then follow the on-device step."
