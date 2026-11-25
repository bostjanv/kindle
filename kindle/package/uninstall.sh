#!/bin/sh
# OTA uninstall script: remove dashboard autostart hook
_FUNCTIONS=/etc/rc.d/functions
[ -f ${_FUNCTIONS} ] && . ${_FUNCTIONS}

update_progressbar 10
[ -f /var/run/dashboard.pid ] && kill "$(cat /var/run/dashboard.pid)" 2>/dev/null
rm -f /etc/rcS.d/S77dashboard /etc/init.d/dashboard
update_progressbar 100
return 0
