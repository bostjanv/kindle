#!/bin/sh
# Kindle Keyboard (K3, FW 3.x) dashboard loop.
#
#   sh /mnt/us/dashboard/dashboard.sh start|stop|restart|refresh|status
#
# Periodically downloads a 600x800 greyscale PNG from the dashboard server
# and draws it with eips. The page-turn buttons (and 5-way left/right) switch
# between the server's screens. Settings: /mnt/us/dashboard/dashboard.conf

DIR=${DASH_DIR:-/mnt/us/dashboard}
CONF=$DIR/dashboard.conf
PIDFILE=${DASH_PIDFILE:-/var/run/dashboard.pid}
RUNCOPY=/tmp/dashboard-run.sh
IMG=/tmp/dashboard.png
SCREEN_FILE=/tmp/dashboard.screen
# Log on the USB drive so it can be read from a PC (writes are simply lost
# while the Kindle is in USB drive mode).
LOG=$DIR/dashboard.log

log() { echo "$(date '+%Y-%m-%d %H:%M:%S') $*" >> $LOG 2>/dev/null; }

load_conf() {
    SERVER_IP=192.168.1.100; SERVER_PORT=8080; SERVER_URL=
    INTERVAL=60; FULL_REFRESH_EVERY=30; STOP_FRAMEWORK=yes; FETCH_TIMEOUT=30
    NEXT_KEYS="191 104 106"; PREV_KEYS="193 105"
    KEY_DEVICES="/dev/input/event0 /dev/input/event1"; DEBUG_KEYS=no
    # dashboard.conf may have been saved with Windows line endings
    [ -f "$CONF" ] && eval "$(tr -d '\r' < "$CONF")"
    [ -n "$SERVER_URL" ] || SERVER_URL="http://$SERVER_IP:$SERVER_PORT/dashboard.png"
}

running() { [ -f $PIDFILE ] && kill -0 "$(cat $PIDFILE)" 2>/dev/null; }

battery() {
    b=$(lipc-get-prop -i com.lab126.powerd battLevel 2>/dev/null)
    [ -n "$b" ] || b=$(gasgauge-info -c 2>/dev/null | tr -dc '0-9')
    echo "$b"
}

screen() { s=$(cat $SCREEN_FILE 2>/dev/null); echo "${s:-0}"; }

url() { echo "$SERVER_URL?screen=$(screen)&battery=$(battery)"; }

wifi_connected() {
    [ "$(lipc-get-prop -s com.lab126.wifid cmState 2>/dev/null)" = "CONNECTED" ]
}

ensure_wifi() {
    wifi_connected && return 0
    log "wifi not connected, enabling"
    lipc-set-prop -i com.lab126.wifid enable 1 2>/dev/null
    i=0
    while [ $i -lt 40 ]; do
        sleep 1
        wifi_connected && return 0
        i=$((i + 1))
    done
    return 1
}

fetch() {
    rm -f $IMG.tmp
    wget -q -O $IMG.tmp "$1" &
    wpid=$!
    ( sleep "$FETCH_TIMEOUT"; kill $wpid 2>/dev/null ) &
    kpid=$!
    # a button press (USR1) interrupts wait early: keep waiting until wget is really done
    wait $wpid
    rc=$?
    while kill -0 $wpid 2>/dev/null; do wait $wpid; rc=$?; done
    kill $kpid 2>/dev/null
    [ $rc -eq 0 ] && [ -s $IMG.tmp ] && mv $IMG.tmp $IMG
}

show_error() {
    # bottom text row of the panel (eips text grid is 50x40 on the K3)
    eips 1 38 "                                                " >/dev/null 2>&1
    eips 1 38 "$1" >/dev/null 2>&1
}

in_list() { for k in $2; do [ "$k" -eq "$1" ] 2>/dev/null && return 0; done; return 1; }

# Reads raw input events (struct input_event, 16 bytes on the K3's 32-bit ARM:
# 8 bytes timestamp, u16 type, u16 code, s32 value) from one device and, on a
# page key press, bumps the screen number and pokes the main loop.
key_listener() {
    dev=$1
    while :; do
        # od -d prints the (octal) offset, then 8 unsigned 16-bit words:
        # time(4 words) type code value_lo value_hi. Compare with test -eq,
        # which is decimal even with od's zero padding.
        set -- $(dd if="$dev" bs=16 count=1 2>/dev/null | od -d | head -n 1)
        [ $# -ge 9 ] || { sleep 1; continue; }
        shift
        type=$5; code=$6; value=$7
        [ "$type" -eq 1 ] && [ "$value" -eq 1 ] || continue   # EV_KEY press only
        code=$(expr "$code" + 0)  # strip zero padding
        [ "$DEBUG_KEYS" = "yes" ] && log "key $code on $dev"
        kill -0 $LOOP_PID 2>/dev/null || exit 0   # dashboard was stopped
        cur=$(screen)
        if in_list $code "$NEXT_KEYS"; then
            echo $((cur + 1)) > $SCREEN_FILE
        elif in_list $code "$PREV_KEYS"; then
            echo $((cur - 1)) > $SCREEN_FILE
        else
            continue
        fi
        kill -USR1 $LOOP_PID 2>/dev/null
    done
}

draw() {  # $1 = full|partial
    if [ "$1" = full ]; then eips -f -g $IMG >/dev/null 2>&1; else eips -g $IMG >/dev/null 2>&1; fi
}

loop() {
    load_conf
    LOOP_PID=$$
    log "started; url=$SERVER_URL interval=$INTERVAL"
    lipc-set-prop com.lab126.powerd preventScreenSaver 1 2>/dev/null
    if [ "$STOP_FRAMEWORK" = "yes" ]; then
        /etc/init.d/framework stop >/dev/null 2>&1
        sleep 2
    fi
    eips -c >/dev/null 2>&1

    LISTENERS=
    for dev in $KEY_DEVICES; do
        [ -e "$dev" ] || continue
        key_listener "$dev" &
        LISTENERS="$LISTENERS $!"
    done
    trap 'kill $LISTENERS $SLEEP_PID 2>/dev/null; exit 0' TERM INT
    # A button press interrupts the sleep below (wait returns early on a trapped signal).
    trap 'WOKEN=1' USR1

    n=0
    fails=0
    while :; do
        WOKEN=
        if fetch "$(url)" || { ensure_wifi && fetch "$(url)"; }; then
            fails=0
            if [ $((n % FULL_REFRESH_EVERY)) -eq 0 ]; then draw full; else draw partial; fi
            n=$((n + 1))
        else
            fails=$((fails + 1))
            log "fetch failed ($fails): $(url)"
            show_error "No dashboard from $SERVER_IP ($fails)"
        fi
        # button pressed while fetching: fetch the new screen right away, with a full refresh
        [ -n "$WOKEN" ] && { n=0; continue; }
        # wake up just after the next INTERVAL boundary so the clock is exact
        now=$(date +%s)
        sleep $((INTERVAL - now % INTERVAL + 1)) &
        SLEEP_PID=$!
        wait $SLEEP_PID
        kill $SLEEP_PID 2>/dev/null
        # switching screens: do a full (flashing) refresh so no ghost of the old screen remains
        [ -n "$WOKEN" ] && n=0
    done
}

start() {
    if running; then echo "already running"; return 0; fi
    # trim the log on the USB drive
    [ -f $LOG ] && [ "$(wc -c < $LOG)" -gt 65536 ] && tail -n 200 $LOG > $LOG.tmp && mv $LOG.tmp $LOG
    # Run from a copy in /tmp: /mnt/us disappears while the Kindle is in USB drive mode.
    cp "$0" $RUNCOPY 2>/dev/null || cp $DIR/dashboard.sh $RUNCOPY
    sh $RUNCOPY __loop >/dev/null 2>&1 &
    echo $! > $PIDFILE
    echo "started (pid $(cat $PIDFILE))"
}

stop() {
    if running; then
        kill "$(cat $PIDFILE)" 2>/dev/null
        sleep 1
    fi
    rm -f $PIDFILE
    # key readers blocked in dd outlive their listener; end them too
    for p in $(pidof dd 2>/dev/null); do
        tr '\0' ' ' < /proc/$p/cmdline 2>/dev/null | grep -q "if=/dev/input/" && kill $p 2>/dev/null
    done
    lipc-set-prop com.lab126.powerd preventScreenSaver 0 2>/dev/null
    load_conf
    if [ "$STOP_FRAMEWORK" = "yes" ] && ! pidof cvm >/dev/null 2>&1; then
        eips -c >/dev/null 2>&1
        eips 15 19 "Restarting Kindle UI..." >/dev/null 2>&1
        /etc/init.d/framework start >/dev/null 2>&1
    fi
    echo "stopped"
}

case "$1" in
    start) start ;;
    stop) stop ;;
    restart) stop; sleep 2; start ;;
    refresh)
        load_conf
        fetch "$(url)" && draw full ;;
    status) running && echo "running (pid $(cat $PIDFILE)), screen $(screen)" || echo "stopped"; tail -5 $LOG 2>/dev/null ;;
    __loop) loop ;;
    *) echo "usage: $0 start|stop|restart|refresh|status"; exit 1 ;;
esac
