#!/bin/sh
# Collects facts about this Kindle into /mnt/us/dashboard/diag.txt (readable over USB).
# Run with Launchpad: Shift D I. Press Next Page a few times while the message is shown.
OUT=/mnt/us/dashboard/diag.txt
{
    echo "== date"; date
    echo "== uname"; uname -a
    echo "== busybox"; busybox 2>&1 | head -2
    echo "== applets"; busybox --list 2>/dev/null | tr '\n' ' '; echo
    for c in od hexdump dd expr pidof tr head wget sed ifconfig; do printf '%s: ' $c; which $c 2>/dev/null || echo MISSING; done
    echo "== /dev/input"; ls -l /dev/input/
    echo "== /proc/bus/input/devices"; cat /proc/bus/input/devices 2>/dev/null
    echo "== wifi"; lipc-get-prop -s com.lab126.wifid cmState 2>&1; ifconfig wlan0 2>&1 | head -3
    echo "== battery"; lipc-get-prop -i com.lab126.powerd battLevel 2>&1
    echo "== power"; lipc-get-prop -s com.lab126.powerd state 2>&1; lipc-get-prop -i com.lab126.powerd preventScreenSaver 2>&1
    echo "== wget"; wget 2>&1 | head -3
    echo "== processes"; ps 2>/dev/null | grep -e dashboard -e launchpad -e cvm -e "dd " | grep -v grep
} > $OUT 2>&1

eips 1 37 "DIAG: press Next Page, Prev Page now  " >/dev/null 2>&1
for dev in /dev/input/event*; do
    (
        # raw bytes of up to 6 events from each device, 8 s max
        dd if=$dev bs=16 count=6 2>/dev/null > /tmp/diag-$(basename $dev).bin &
        p=$!; sleep 8; kill $p 2>/dev/null
    ) &
done
sleep 9
for f in /tmp/diag-event*.bin; do
    echo "== raw $f ($(wc -c < $f) bytes)" >> $OUT
    hexdump -C $f >> $OUT 2>&1
done
rm -f /tmp/diag-event*.bin
eips 1 37 "DIAG: done, see dashboard/diag.txt    " >/dev/null 2>&1
