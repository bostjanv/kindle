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
PIDFILE=${DASH_PIDFILE:-/tmp/dashboard.pid}
RUNCOPY=/tmp/dashboard-run.sh
IMG=/tmp/dashboard.png
SCREEN_FILE=/tmp/dashboard.screen
SLEEP_FILE=/tmp/dashboard.sleep
TIME_FILE=/tmp/dashboard.time
# Log on the USB drive so it can be read from a PC (writes are simply lost
# while the Kindle is in USB drive mode).
LOG=$DIR/dashboard.log

log() { echo "$(date '+%Y-%m-%d %H:%M:%S') $*" >> $LOG 2>/dev/null; }

load_conf() {
    SERVER_IP=192.168.1.100; SERVER_PORT=8080; SERVER_URL=
    INTERVAL=60; FULL_REFRESH_EVERY=30; STOP_FRAMEWORK=yes; FETCH_TIMEOUT=20
    NEXT_KEYS="191 104 106"; PREV_KEYS="109 193 105"
    KEY_DEVICES="/dev/input/event0 /dev/input/event1"; DEBUG_KEYS=no
    # dashboard.conf may have been saved with Windows line endings
    [ -f "$CONF" ] && eval "$(tr -d '\r' < "$CONF")"
    [ -n "$SERVER_URL" ] || SERVER_URL="http://$SERVER_IP:$SERVER_PORT/dashboard.png"
    TIME_URL="${SERVER_URL%/*}/time"
}

# PIDs of every dashboard loop process (the loop shell and its listener
# subshells all share its command line), found by scanning /proc rather than
# trusting the pid file, so copies started twice or left over are caught too.
loop_pids() {
    for d in /proc/[0-9]*; do
        case "$(tr '\0' ' ' < $d/cmdline 2>/dev/null)" in
            *"dashboard-run.sh __loop"*) echo "${d#/proc/}" ;;
        esac
    done
}

running() { [ -n "$(loop_pids)" ]; }

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

# download $1 to $2, giving up after FETCH_TIMEOUT seconds
download() {
    rm -f "$2"
    wget -q -O "$2" "$1" &
    wpid=$!
    ( sleep "$FETCH_TIMEOUT"; kill $wpid 2>/dev/null ) &
    kpid=$!
    # a button press (USR1) interrupts wait early: keep waiting until wget is really done
    wait $wpid
    rc=$?
    while kill -0 $wpid 2>/dev/null; do wait $wpid; rc=$?; done
    kill $kpid 2>/dev/null
    [ $rc -eq 0 ] && [ -s "$2" ]
}

fetch() { download "$1" $IMG.tmp && mv $IMG.tmp $IMG; }

# Seconds the server's clock is ahead of ours, kept in CLOCK_OFFSET. The
# Kindle's clock drifts (nothing sets it while the framework is stopped), and
# the server draws the clock from its own time, so refreshes are timed by the
# server's clock. On failure the previous offset is kept.
sync_clock() {
    download "$TIME_URL" $TIME_FILE || return 1
    t=$(tr -dc '0-9' < $TIME_FILE)
    [ -n "$t" ] || return 1
    o=$((t - $(date +%s)))
    [ "$o" != "$CLOCK_OFFSET" ] && log "clock offset to server: ${o}s"
    CLOCK_OFFSET=$o
}

show_error() {
    # bottom text row of the panel (eips text grid is 50x40 on the K3)
    eips 1 38 "                                                " >/dev/null 2>&1
    eips 1 38 "$1" >/dev/null 2>&1
}

in_list() { for k in $2; do [ "$k" -eq "$1" ] 2>/dev/null && return 0; done; return 1; }

# "bf" -> 191. Pure shell: the K3's busybox 1.7.2 has no od, and its
# arithmetic may not accept 0x literals.
h2d() {
    _r=0; _s=$1
    while [ -n "$_s" ]; do
        _c=${_s%"${_s#?}"}; _s=${_s#?}
        case $_c in
            [0-9]) _d=$_c ;; a|A) _d=10 ;; b|B) _d=11 ;; c|C) _d=12 ;;
            d|D) _d=13 ;; e|E) _d=14 ;; f|F) _d=15 ;; *) _d=0 ;;
        esac
        _r=$((_r * 16 + _d))
    done
    echo $_r
}

# Reads raw input events (struct input_event, 16 bytes on the K3's 32-bit ARM:
# 8 bytes timestamp, u16 type, u16 code, s32 value, little-endian) from one
# device and, on a page key press, bumps the screen number and wakes the loop.
key_listener() {
    dev=$1
    while :; do
        # hexdump -C: offset, 16 hex bytes, |ascii|. Drop the offset and the
        # 8 timestamp bytes, leaving type (2 bytes), code (2), value (4), LE.
        set -- $(dd if="$dev" bs=16 count=1 2>/dev/null | hexdump -C | head -n 1)
        [ $# -ge 17 ] || { sleep 1; continue; }
        shift 9
        type=$(h2d "$2$1"); code=$(h2d "$4$3"); value=$(h2d "$5")
        [ "$type" -eq 1 ] && [ "$value" -eq 1 ] || continue   # EV_KEY press only
        # stale listener from a stopped/restarted dashboard: leave quietly
        [ "$(cat $PIDFILE 2>/dev/null)" = "$LOOP_PID" ] || exit 0
        [ "$DEBUG_KEYS" = "yes" ] && log "key $code on $dev (loop $LOOP_PID)"
        cur=$(screen)
        if in_list $code "$NEXT_KEYS"; then
            echo $((cur + 1)) > $SCREEN_FILE
        elif in_list $code "$PREV_KEYS"; then
            echo $((cur - 1)) > $SCREEN_FILE
        else
            continue
        fi
        kill -USR1 $LOOP_PID 2>/dev/null
        # also end the loop's sleep directly, in case this old ash doesn't let
        # a trapped signal interrupt `wait`
        [ -f $SLEEP_FILE ] && kill "$(cat $SLEEP_FILE)" 2>/dev/null
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
    # A button press ends the sleep below (the listener kills it, and USR1
    # interrupts `wait` where the shell allows). The trap only has to exist so
    # USR1 doesn't terminate the loop; what to draw is decided by comparing
    # screen numbers, so extra or late wake-ups can't cause a second redraw.
    trap ':' USR1

    # start() writes the pid file right after launching us; give it a moment
    i=0
    while [ ! -f $PIDFILE ] && [ $i -lt 10 ]; do sleep 1; i=$((i + 1)); done

    n=0
    fails=0
    CLOCK_OFFSET=0
    drawn=
    while :; do
        # not the official instance (stopped, or a duplicate start)? quit
        if [ "$(cat $PIDFILE 2>/dev/null)" != "$LOOP_PID" ]; then
            log "loop $LOOP_PID is not the current instance; exiting"
            kill $LISTENERS 2>/dev/null
            exit 0
        fi
        cur=$(screen)
        u="$SERVER_URL?screen=$cur&battery=$(battery)"
        if fetch "$u" || { ensure_wifi && fetch "$u"; }; then
            fails=0
            # full (flashing) refresh on a screen switch, so no ghost of the old
            # screen remains, and every FULL_REFRESH_EVERY updates
            if [ "$cur" != "$drawn" ] || [ $((n % FULL_REFRESH_EVERY)) -eq 0 ]; then
                draw full
                n=0
            else
                draw partial
            fi
            n=$((n + 1))
            drawn=$cur
            sync_clock
        else
            fails=$((fails + 1))
            log "fetch failed ($fails): $u"
            show_error "No dashboard from $SERVER_IP ($fails)"
        fi
        # button pressed while fetching: get the new screen right away
        [ "$(screen)" != "$cur" ] && continue
        # wake up just after the next INTERVAL boundary of the server's clock
        # so the clock is exact (2 s margin: the offset is in whole seconds and
        # measured over the network)
        now=$(($(date +%s) + CLOCK_OFFSET))
        sleep $((INTERVAL - now % INTERVAL + 2)) &
        SLEEP_PID=$!
        echo $SLEEP_PID > $SLEEP_FILE
        wait $SLEEP_PID
        rm -f $SLEEP_FILE
        kill $SLEEP_PID 2>/dev/null
    done
}

start() {
    # lock: the hotkey and the boot-time autostart may run start at the same moment
    if ! mkdir /tmp/dashboard.lock 2>/dev/null; then echo "already starting"; return 0; fi
    if running; then rmdir /tmp/dashboard.lock; echo "already running"; return 0; fi
    # one-shot diagnostics: put an empty file named RUN_DIAG in the dashboard folder
    if [ -f $DIR/RUN_DIAG ]; then
        rm -f $DIR/RUN_DIAG
        sh $DIR/diag.sh
    fi
    # trim the log on the USB drive
    [ -f $LOG ] && [ "$(wc -c < $LOG)" -gt 65536 ] && tail -n 200 $LOG > $LOG.tmp && mv $LOG.tmp $LOG
    # Run from a copy in /tmp: /mnt/us disappears while the Kindle is in USB drive mode.
    cp "$0" $RUNCOPY 2>/dev/null || cp $DIR/dashboard.sh $RUNCOPY
    sh $RUNCOPY __loop >/dev/null 2>&1 &
    echo $! > $PIDFILE
    rmdir /tmp/dashboard.lock
    echo "started (pid $(cat $PIDFILE))"
}

stop() {
    rm -f $PIDFILE   # makes any loop still alive exit at its next check
    pids=$(loop_pids)
    if [ -n "$pids" ]; then
        kill $pids 2>/dev/null
        # end their sleeps too, so old shells run their exit trap right away
        [ -f $SLEEP_FILE ] && kill "$(cat $SLEEP_FILE)" 2>/dev/null
        sleep 1
        pids=$(loop_pids)
        [ -n "$pids" ] && kill -9 $pids 2>/dev/null
    fi
    rm -f $SLEEP_FILE
    rmdir /tmp/dashboard.lock 2>/dev/null
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
    status) running && echo "running (pids $(loop_pids | tr '\n' ' ')), screen $(screen)" || echo "stopped"; tail -5 $LOG 2>/dev/null ;;
    __loop) loop ;;
    *) echo "usage: $0 start|stop|restart|refresh|status"; exit 1 ;;
esac
