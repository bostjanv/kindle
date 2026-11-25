#!/bin/sh
# OTA install script: dashboard autostart hook
_FUNCTIONS=/etc/rc.d/functions
[ -f ${_FUNCTIONS} ] && . ${_FUNCTIONS}

HACKNAME="dashboard"
SLEVEL="77"

update_progressbar 10
cp -f ${HACKNAME}-init /etc/init.d/${HACKNAME} || return 1
chmod +x /etc/init.d/${HACKNAME}
update_progressbar 50
[ -h /etc/rcS.d/S${SLEVEL}${HACKNAME} ] || ln -fs /etc/init.d/${HACKNAME} /etc/rcS.d/S${SLEVEL}${HACKNAME}
update_progressbar 90
rm -f ${HACKNAME}-init
update_progressbar 100
return 0
