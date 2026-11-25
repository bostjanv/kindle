#!/bin/sh
# Builds the autostart OTA packages with KindleTool (signed with the jailbreak key).
# Usage: build.sh [device]   (default k3gb = Kindle Keyboard 3G EU, B00A)
set -e
DEV=${1:-k3gb}
KT=${KINDLETOOL:-kindletool}
cd "$(dirname "$0")"
mkdir -p out
$KT create ota -d "$DEV" install.sh dashboard-init "out/Update_dashboard_${DEV}_install.bin" </dev/null
$KT create ota -d "$DEV" uninstall.sh "out/Update_dashboard_${DEV}_uninstall.bin" </dev/null
ls -l out
