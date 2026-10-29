#!/system/bin/sh

MODDIR=${0%/*}
ONYX_ROOT=/data/adb/peridot-essential
ONYX_WATCHDOG_LOCK="$ONYX_ROOT/watchdog.lock"
ONYX_WATCHDOG_PID="$(cat "$ONYX_ROOT/watchdog.pid" 2>/dev/null)"
ONYX_PACKAGE=io.github.davvv98.peridotessential
ONYX_NATIVE_MODE=0
[ -f "$MODDIR/rom-native-mode" ] && ONYX_NATIVE_MODE=1

# Release an explicit Sconfig override while the daemon is still running.
if [ -x "$MODDIR/bin/peridot_ctl" ] && command -v timeout >/dev/null 2>&1; then
    timeout 8 "$MODDIR/bin/peridot_ctl" SET cpu.sconfig -1 \
        >"$ONYX_ROOT/sconfig-uninstall.log" 2>&1 || true
fi

case "$ONYX_WATCHDOG_PID" in
    ''|*[!0-9]*) ;;
    *) kill "$ONYX_WATCHDOG_PID" 2>/dev/null ;;
esac

for ONYX_PROCESS_NAME in peridot_daemon peridot_essentiald; do
    for ONYX_PID in $(pidof "$ONYX_PROCESS_NAME" 2>/dev/null); do
        kill "$ONYX_PID" 2>/dev/null
    done
done
sleep 1
for ONYX_PROCESS_NAME in peridot_daemon peridot_essentiald; do
    for ONYX_PID in $(pidof "$ONYX_PROCESS_NAME" 2>/dev/null); do
        kill -9 "$ONYX_PID" 2>/dev/null
    done
done

rm -f "$ONYX_ROOT/watchdog.pid" "$ONYX_WATCHDOG_LOCK/pid" "$ONYX_WATCHDOG_LOCK/boot_id"
rmdir "$ONYX_WATCHDOG_LOCK" 2>/dev/null
rm -f "$ONYX_ROOT/bin/peridot_daemon" "$ONYX_ROOT/bin/peridot_ctl"
rmdir "$ONYX_ROOT/bin" 2>/dev/null
rm -f "$ONYX_ROOT/backend.log" "$ONYX_ROOT/backend.log.1"

if [ "$ONYX_NATIVE_MODE" -eq 1 ]; then
    # The module APK is an Android package-manager update over the matching ROM
    # system app. Revert only that update, keep package data, and return daemon
    # ownership to init. The native state directory is never removed.
    /system/bin/cmd package uninstall-system-updates "$ONYX_PACKAGE" \
        >"$ONYX_ROOT/native-app-rollback.log" 2>&1 || true
    /system/bin/pm enable --user 0 "$ONYX_PACKAGE" >/dev/null 2>&1 || true
    for ONYX_NATIVE_SERVICE in peridot_daemon peridot_essentiald; do
        if [ -n "$(getprop "init.svc.$ONYX_NATIVE_SERVICE" 2>/dev/null)" ]; then
            /system/bin/setprop ctl.start "$ONYX_NATIVE_SERVICE" 2>/dev/null || true
        fi
    done
fi

# Standalone installations keep state.conf and the ordinary Android app so
# removing the root module does not erase user data. The app can be uninstalled
# normally from Android.
exit 0
