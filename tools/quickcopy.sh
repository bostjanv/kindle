#!/bin/bash
# Waits for the Kindle drive, copies a deployment stage, verifies every file by
# reading it back, and only then ejects (so the Kindle leaves USB drive mode).
#
#   tools/quickcopy.sh jailbreak|launchpad|dashboard|autostart
#   SERVER_IP=... SERVER_PORT=... tools/quickcopy.sh dashboard
#   tools/quickcopy.sh launchpad dashboard     # several stages in one go
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEV=${KINDLE_DEV:-k3gb}
DL=$ROOT/downloads/x
MNT=/tmp/kindle-usbdisk; mkdir -p "$MNT"
USBDISK=$ROOT/tools/usbdisk/usbdisk
export MTOOLSRC=$(mktemp)
printf 'drive k: file="%s/disk" partition=1\nmtools_skip_check=1\n' "$MNT" > "$MTOOLSRC"
TMP=$(mktemp -d)

# stage -> list of "local_source|kindle_path"
files=()
[ $# -gt 0 ] || set -- help
for stage in "$@"; do
case "$stage" in
    jailbreak) files+=("$DL/kindle-jailbreak-0.13.N/Update_jailbreak_0.13.N_${DEV}_install.bin|Update_jailbreak_0.13.N_${DEV}_install.bin") ;;
    launchpad) files+=("$DL/lpad-pkg-001d/update_launchpad_0.0.1d_${DEV}_install.bin|update_launchpad_0.0.1d_${DEV}_install.bin") ;;
    dashboard)
        sed "s/^SERVER_IP=.*/SERVER_IP=${SERVER_IP:-192.168.1.100}/; s/^SERVER_PORT=.*/SERVER_PORT=${SERVER_PORT:-8080}/" \
            "$ROOT/kindle/dashboard/dashboard.conf" > "$TMP/dashboard.conf"
        files+=("$ROOT/kindle/dashboard/dashboard.sh|dashboard/dashboard.sh"
                "$TMP/dashboard.conf|dashboard/dashboard.conf"
                "$ROOT/kindle/launchpad/dashboard.ini|launchpad/dashboard.ini") ;;
    autostart)
        [ -f "$ROOT/kindle/package/out/Update_dashboard_${DEV}_install.bin" ] || \
            KINDLETOOL=${KINDLETOOL:-$ROOT/build/KindleTool/KindleTool/Release/kindletool} "$ROOT/kindle/package/build.sh" "$DEV" >/dev/null
        files+=("$ROOT/kindle/package/out/Update_dashboard_${DEV}_install.bin|Update_dashboard_${DEV}_install.bin") ;;
    *) sed -n '2,7p' "$0"; exit 1 ;;
esac
done

echo "waiting for Kindle drive..."
until lsusb | grep -q 1949: && sudo USBDISK_PROBE=1 "$USBDISK" >/dev/null 2>&1; do sleep 1; done
echo "$(date +%T) drive ready"
sudo fusermount3 -u "$MNT" 2>/dev/null
sudo "$USBDISK" -s -o allow_other "$MNT" 2>/dev/null
fail() { echo "FAILED: $*; NOT ejecting"; sudo fusermount3 -u "$MNT"; exit 1; }
for _ in $(seq 30); do minfo k: >/dev/null 2>&1 && break; sleep 0.5; done
minfo k: >/dev/null 2>&1 || fail "cannot read the FAT filesystem"
echo "drive root:"; mdir -a -b k:/ | sed "s|^|  |"

for f in "${files[@]}"; do
    src=${f%%|*}; dst=${f#*|}
    dir=$(dirname "$dst"); [ "$dir" = . ] || mmd "k:/$dir" 2>/dev/null
    mcopy -o "$src" "k:/$dst" || fail "copy $dst"
done
sync
for f in "${files[@]}"; do
    src=${f%%|*}; dst=${f#*|}
    rm -f "$TMP/verify"
    mcopy -o "k:/$dst" "$TMP/verify" >/dev/null 2>&1 && cmp -s "$TMP/verify" "$src" || fail "verify $dst"
    echo "  ok  $dst"
done
echo "update packages on the drive:"; mdir -a -b k:/ | grep -i '\.bin$' || echo "  (none)"
sudo fusermount3 -u "$MNT"; sleep 1
sudo USBDISK_EJECT=1 "$USBDISK" 2>&1 | grep eject
echo "$(date +%T) done"
