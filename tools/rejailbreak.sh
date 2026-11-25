#!/bin/bash
# Waits for the Kindle drive, puts the jailbreak on it as fast as possible,
# verifies it, and only then ejects.
ROOT=$(cd "$(dirname "$0")/.." && pwd)
F=Update_jailbreak_0.13.N_k3gb_install.bin
SRC="$ROOT/downloads/x/kindle-jailbreak-0.13.N/$F"
MNT=/tmp/kindle-usbdisk; mkdir -p $MNT
export MTOOLSRC=$(mktemp); printf 'drive k: file="%s/disk" partition=1\nmtools_skip_check=1\n' $MNT > $MTOOLSRC
echo "waiting for Kindle drive..."
until lsusb | grep -q 1949: && sudo USBDISK_PROBE=1 "$ROOT/tools/usbdisk/usbdisk" >/dev/null 2>&1; do sleep 1; done
echo "$(date +%T) drive ready"
sudo fusermount3 -u $MNT 2>/dev/null
sudo "$ROOT/tools/usbdisk/usbdisk" -s -o allow_other $MNT
ok=
for i in $(seq 30); do
    if minfo k: >/dev/null 2>&1; then ok=1; break; fi
    sleep 0.5
done
[ -n "$ok" ] || { echo "FAILED: cannot read the FAT filesystem; NOT ejecting"; sudo fusermount3 -u $MNT; exit 1; }
for old in update_launchpad_0.0.1d_k3gb_install.bin Update_jailbreak_0.13.N_k3gb-3.0-to-3.2_install.bin; do mdel "k:/$old" 2>/dev/null && echo "removed leftover $old"; done
mcopy -o "$SRC" "k:/$F" || { echo "FAILED: copy; NOT ejecting"; sudo fusermount3 -u $MNT; exit 1; }
sync
rm -f /tmp/claude-1000/v3.bin
if mcopy -o "k:/$F" /tmp/claude-1000/v3.bin && cmp -s /tmp/claude-1000/v3.bin "$SRC"; then
    echo "$(date +%T) jailbreak copied + verified"
else
    echo "FAILED: verification; NOT ejecting"; sudo fusermount3 -u $MNT; exit 1
fi
mdir -a k:/ | grep -iE "\.bin"
sudo fusermount3 -u $MNT; sleep 1
sudo USBDISK_EJECT=1 "$ROOT/tools/usbdisk/usbdisk" 2>&1 | grep eject
echo "$(date +%T) done - run Update Your Kindle now"
