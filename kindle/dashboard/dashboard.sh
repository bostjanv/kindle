#!/bin/sh
# Kindle Keyboard (K3, FW 3.x) dashboard loop.
#
#   sh /mnt/us/dashboard/dashboard.sh start|stop|restart|refresh|status
#
# Periodically downloads a 600x800 greyscale PNG from the dashboard server
# and draws it with eips. Settings: /mnt/us/dashboard/dashboard.conf

DIR=/mnt/us/dashboard
CONF=$DIR/dashboard.conf
PIDFILE=/var/run/dashboard.pid
RUNCOPY=/tmp/dashboard-run.sh
IMG=/tmp/dashboard.png
LOG=/tmp/dashboard.log

log() { echo "$(date '+%Y-%m-%d %H:%M:%S') $*" >> $LOG; }

load_conf() {
    SERVER_IP=192.168.1.100; SERVER_PORT=8080; SERVER_URL=
    INTERVAL=60; FULL_REFRESH_EVERY=30; STOP_FRAMEWORK=yes; FETCH_TIMEOUT=30
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
    wait $wpid
    rc=$?
    kill $kpid 2>/dev/null
    [ $rc -eq 0 ] && [ -s $IMG.tmp ] && mv $IMG.tmp $IMG
}

show_error() {
    # bottom text row of the panel (eips text grid is 50x40 on the K3)
    eips 1 38 "                                                " >/dev/null 2>&1
    eips 1 38 "$1" >/dev/null 2>&1
}

loop() {
    load_conf
    log "started; url=$SERVER_URL interval=$INTERVAL"
    lipc-set-prop com.lab126.powerd preventScreenSaver 1 2>/dev/null
    if [ "$STOP_FRAMEWORK" = "yes" ]; then
        /etc/init.d/framework stop >/dev/null 2>&1
        sleep 2
    fi
    eips -c >/dev/null 2>&1
    n=0
    fails=0
    while :; do
        url="$SERVER_URL?battery=$(battery)"
        if fetch "$url" || { ensure_wifi && fetch "$url"; }; then
            fails=0
            if [ $((n % FULL_REFRESH_EVERY)) -eq 0 ]; then
                eips -f -g $IMG >/dev/null 2>&1
            else
                eips -g $IMG >/dev/null 2>&1
            fi
            n=$((n + 1))
        else
            fails=$((fails + 1))
            log "fetch failed ($fails): $url"
            show_error "No dashboard from $SERVER_IP ($fails)"
        fi
        # wake up just after the next INTERVAL boundary so the clock is exact
        now=$(date +%s)
        sleep $((INTERVAL - now % INTERVAL + 1))
    done
}

start() {
    if running; then echo "already running"; return 0; fi
    # Run from a copy in /tmp: /mnt/us disappears while the Kindle is in USB drive mode.
    cp "$0" $RUNCOPY 2>/dev/null || cp $DIR/dashboard.sh $RUNCOPY
    sh $RUNCOPY __loop >/dev/null 2>&1 &
    echo $! > $PIDFILE
    echo "started (pid $(cat $PIDFILE))"
}

stop() {
    if running; then
        kill "$(cat $PIDFILE)" 2>/dev/null
        # the loop may be sleeping or inside wget; clean up its children
        pkill -f "$RUNCOPY __loop" 2>/dev/null
    fi
    rm -f $PIDFILE
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
        fetch "$SERVER_URL?battery=$(battery)" && eips -f -g $IMG >/dev/null 2>&1 ;;
    status) running && echo "running (pid $(cat $PIDFILE))" || echo "stopped"; tail -5 $LOG 2>/dev/null ;;
    __loop) loop ;;
    *) echo "usage: $0 start|stop|restart|refresh|status"; exit 1 ;;
esac
