#!/system/bin/sh

MODDIR=${0%/*}
PERIDOT_ROOT=/data/adb/peridot-essential
PERIDOT_WATCHDOG_LOCK="$PERIDOT_ROOT/watchdog.lock"
PERIDOT_WATCHDOG_PID="$(cat "$PERIDOT_ROOT/watchdog.pid" 2>/dev/null)"
PERIDOT_PACKAGE=io.github.davvv98.peridotessential
PERIDOT_NATIVE_MODE=0
[ -f "$MODDIR/rom-native-mode" ] && PERIDOT_NATIVE_MODE=1

# Release an explicit Sconfig override while the daemon is still running.
if [ -x "$MODDIR/bin/peridot_ctl" ] && command -v timeout >/dev/null 2>&1; then
    timeout 8 "$MODDIR/bin/peridot_ctl" SET cpu.sconfig -1 \
        >"$PERIDOT_ROOT/sconfig-uninstall.log" 2>&1 || true
fi

case "$PERIDOT_WATCHDOG_PID" in
    ''|*[!0-9]*) ;;
    *) kill "$PERIDOT_WATCHDOG_PID" 2>/dev/null ;;
esac

for PERIDOT_PROCESS_NAME in peridot_daemon peridot_essentiald; do
    for PERIDOT_PID in $(pidof "$PERIDOT_PROCESS_NAME" 2>/dev/null); do
        kill "$PERIDOT_PID" 2>/dev/null
    done
done
sleep 1
for PERIDOT_PROCESS_NAME in peridot_daemon peridot_essentiald; do
    for PERIDOT_PID in $(pidof "$PERIDOT_PROCESS_NAME" 2>/dev/null); do
        kill -9 "$PERIDOT_PID" 2>/dev/null
    done
done

rm -f "$PERIDOT_ROOT/watchdog.pid" "$PERIDOT_WATCHDOG_LOCK/pid" "$PERIDOT_WATCHDOG_LOCK/boot_id"
rmdir "$PERIDOT_WATCHDOG_LOCK" 2>/dev/null
rm -f "$PERIDOT_ROOT/bin/peridot_daemon" "$PERIDOT_ROOT/bin/peridot_ctl"
rmdir "$PERIDOT_ROOT/bin" 2>/dev/null
rm -f "$PERIDOT_ROOT/backend.log" "$PERIDOT_ROOT/backend.log.1"

if [ "$PERIDOT_NATIVE_MODE" -eq 1 ]; then
    # The module APK is an Android package-manager update over the matching ROM
    # system app. Revert only that update, keep package data, and return daemon
    # ownership to init. The native state directory is never removed.
    /system/bin/cmd package uninstall-system-updates "$PERIDOT_PACKAGE" \
        >"$PERIDOT_ROOT/native-app-rollback.log" 2>&1 || true
    /system/bin/pm enable --user 0 "$PERIDOT_PACKAGE" >/dev/null 2>&1 || true
    for PERIDOT_NATIVE_SERVICE in peridot_daemon peridot_essentiald; do
        if [ -n "$(getprop "init.svc.$PERIDOT_NATIVE_SERVICE" 2>/dev/null)" ]; then
            /system/bin/setprop ctl.start "$PERIDOT_NATIVE_SERVICE" 2>/dev/null || true
        fi
    done
fi

# Standalone installations keep state.conf and the ordinary Android app so
# removing the root module does not erase user data. The app can be uninstalled
# normally from Android.
exit 0
