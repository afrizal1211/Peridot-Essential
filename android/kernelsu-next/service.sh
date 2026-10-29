#!/system/bin/sh

MODDIR=${0%/*}
ONYX_ROOT=/data/adb/peridot-essential
ONYX_NATIVE_STATE_ROOT=/data/system/peridot-essential
ONYX_DAEMON="$MODDIR/bin/peridot_daemon"
ONYX_LOG="$ONYX_ROOT/backend.log"
ONYX_LOG_OLD="$ONYX_ROOT/backend.log.1"
ONYX_WATCHDOG_PID="$ONYX_ROOT/watchdog.pid"
ONYX_WATCHDOG_LOCK="$ONYX_ROOT/watchdog.lock"
ONYX_WATCHDOG_BOOT_ID="$ONYX_WATCHDOG_LOCK/boot_id"
ONYX_SERVICE_PATH="$MODDIR/service.sh"
ONYX_CURRENT_BOOT_ID="$(cat /proc/sys/kernel/random/boot_id 2>/dev/null)"
ONYX_PACKAGE=io.github.davvv98.peridotessential

export ONYX_REVERSE_IPC=1
export ONYX_LOOPBACK_IPC=1

peridot_native_backend_present() {
    [ -f "$MODDIR/rom-native-mode" ] && return 0

    for ONYX_NATIVE_BINARY in \
        /product/bin/peridot_daemon \
        /system_ext/bin/peridot_daemon \
        /system/bin/peridot_daemon \
        /vendor/bin/peridot_daemon \
        /odm/bin/peridot_daemon; do
        [ -x "$ONYX_NATIVE_BINARY" ] && return 0
    done

    for ONYX_NATIVE_SERVICE in peridot_daemon peridot_essentiald; do
        [ -n "$(getprop "init.svc.$ONYX_NATIVE_SERVICE" 2>/dev/null)" ] && return 0
    done
    return 1
}

ONYX_NATIVE_MODE=0
if peridot_native_backend_present; then
    ONYX_NATIVE_MODE=1
    export ONYX_STATE_DIR="$ONYX_NATIVE_STATE_ROOT"
else
    export ONYX_STATE_DIR="$ONYX_ROOT"
fi

umask 077
mkdir -p "$ONYX_ROOT"
chmod 0700 "$ONYX_ROOT" 2>/dev/null
if [ "$ONYX_NATIVE_MODE" -eq 1 ]; then
    mkdir -p "$ONYX_NATIVE_STATE_ROOT"
    chmod 0700 "$ONYX_NATIVE_STATE_ROOT" 2>/dev/null
    /system/bin/restorecon -RF "$ONYX_NATIVE_STATE_ROOT" 2>/dev/null || \
        /system/bin/restorecon -R "$ONYX_NATIVE_STATE_ROOT" 2>/dev/null || true
fi

ONYX_LOG_BYTES=0
if [ -f "$ONYX_LOG" ]; then
    ONYX_LOG_BYTES="$(wc -c <"$ONYX_LOG" 2>/dev/null)"
fi
case "$ONYX_LOG_BYTES" in
    ''|*[!0-9]*) ;;
    *)
        if [ "$ONYX_LOG_BYTES" -gt 1048576 ]; then
            mv -f "$ONYX_LOG" "$ONYX_LOG_OLD"
        fi
        ;;
esac

# mkdir is an atomic early-boot lock and prevents duplicate watchdog loops.
if ! mkdir "$ONYX_WATCHDOG_LOCK" 2>/dev/null; then
    ONYX_EXISTING_PID="$(cat "$ONYX_WATCHDOG_LOCK/pid" 2>/dev/null)"
    ONYX_EXISTING_BOOT_ID="$(cat "$ONYX_WATCHDOG_BOOT_ID" 2>/dev/null)"
    ONYX_EXISTING_CMDLINE=""
    if [ -n "$ONYX_CURRENT_BOOT_ID" ] \
        && [ "$ONYX_EXISTING_BOOT_ID" = "$ONYX_CURRENT_BOOT_ID" ] \
        && [ "$ONYX_EXISTING_PID" != "$$" ] \
        && kill -0 "$ONYX_EXISTING_PID" 2>/dev/null; then
        ONYX_EXISTING_CMDLINE="$(tr '\000' ' ' <"/proc/$ONYX_EXISTING_PID/cmdline" 2>/dev/null)"
    fi
    case "$ONYX_EXISTING_CMDLINE" in
        *"$ONYX_SERVICE_PATH"*) exit 0 ;;
    esac

    rm -f "$ONYX_WATCHDOG_LOCK/pid" "$ONYX_WATCHDOG_BOOT_ID"
    rmdir "$ONYX_WATCHDOG_LOCK" 2>/dev/null || exit 0
    mkdir "$ONYX_WATCHDOG_LOCK" 2>/dev/null || exit 0
fi

printf '%s\n' "$$" >"$ONYX_WATCHDOG_PID"
printf '%s\n' "$$" >"$ONYX_WATCHDOG_LOCK/pid"
printf '%s\n' "$ONYX_CURRENT_BOOT_ID" >"$ONYX_WATCHDOG_BOOT_ID"

# Remove watchdogs left by older development packages of this same module.
for ONYX_PROC in /proc/[0-9]*; do
    ONYX_PID="${ONYX_PROC##*/}"
    [ "$ONYX_PID" = "$$" ] && continue
    ONYX_CMDLINE="$(tr '\000' ' ' <"$ONYX_PROC/cmdline" 2>/dev/null)"
    case "$ONYX_CMDLINE" in
        *"$ONYX_SERVICE_PATH"*)
            kill -9 "$ONYX_PID" 2>/dev/null
            ;;
    esac
done

peridot_cleanup() {
    ONYX_OWNER="$(cat "$ONYX_WATCHDOG_LOCK/pid" 2>/dev/null)"
    if [ "$ONYX_OWNER" = "$$" ]; then
        rm -f "$ONYX_WATCHDOG_PID" "$ONYX_WATCHDOG_LOCK/pid" "$ONYX_WATCHDOG_BOOT_ID"
        rmdir "$ONYX_WATCHDOG_LOCK" 2>/dev/null
    fi
}

trap peridot_cleanup EXIT
trap 'exit 0' HUP INT TERM

peridot_stop_native_services() {
    [ "$ONYX_NATIVE_MODE" -eq 1 ] || return 0

    for ONYX_NATIVE_SERVICE in peridot_daemon peridot_essentiald; do
        ONYX_NATIVE_STATE="$(getprop "init.svc.$ONYX_NATIVE_SERVICE" 2>/dev/null)"
        case "$ONYX_NATIVE_STATE" in
            ''|stopped) ;;
            *) /system/bin/setprop ctl.stop "$ONYX_NATIVE_SERVICE" 2>/dev/null || true ;;
        esac
    done

    ONYX_STOP_ATTEMPT=0
    while [ "$ONYX_STOP_ATTEMPT" -lt 10 ]; do
        ONYX_NATIVE_RUNNING=0
        for ONYX_NATIVE_SERVICE in peridot_daemon peridot_essentiald; do
            ONYX_NATIVE_STATE="$(getprop "init.svc.$ONYX_NATIVE_SERVICE" 2>/dev/null)"
            case "$ONYX_NATIVE_STATE" in
                starting|running|restarting|stopping) ONYX_NATIVE_RUNNING=1 ;;
            esac
        done
        [ "$ONYX_NATIVE_RUNNING" -eq 1 ] || return 0
        ONYX_STOP_ATTEMPT=$((ONYX_STOP_ATTEMPT + 1))
        sleep 1
    done
    return 1
}

peridot_kill_existing_daemons() {
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
}

peridot_resolve_app_uid() {
    ONYX_PACKAGE_ENTRY="$(/system/bin/pm list packages -U "$ONYX_PACKAGE" 2>/dev/null \
        | sed -n "s/^package:$ONYX_PACKAGE[[:space:]]*uid:\([0-9][0-9]*\).*$/\1/p" \
        | head -n 1)"
    case "$ONYX_PACKAGE_ENTRY" in
        ''|*[!0-9]*) ;;
        *)
            if [ "$ONYX_PACKAGE_ENTRY" -ge 10000 ] 2>/dev/null; then
                printf '%s\n' "$ONYX_PACKAGE_ENTRY"
                return 0
            fi
            ;;
    esac

    ONYX_PACKAGE_ENTRY="$(/system/bin/dumpsys package "$ONYX_PACKAGE" 2>/dev/null \
        | sed -n 's/.*userId=\([0-9][0-9]*\).*/\1/p' \
        | head -n 1)"
    case "$ONYX_PACKAGE_ENTRY" in
        ''|*[!0-9]*) return 1 ;;
        *)
            [ "$ONYX_PACKAGE_ENTRY" -ge 10000 ] 2>/dev/null || return 1
            printf '%s\n' "$ONYX_PACKAGE_ENTRY"
            ;;
    esac
}

# KernelSU/Magisk late-start scripts may run while framework/vendor services
# are still publishing their boot defaults. Start the backend only after the
# completed Android boot so every persisted Peridot setting wins that race.
while [ "$(getprop sys.boot_completed 2>/dev/null)" != "1" ]; do
    sleep 1
done

if [ "$ONYX_NATIVE_MODE" -eq 1 ]; then
    echo "ONYX_MODULE_MODE rom_native_update state_dir=$ONYX_STATE_DIR" >>"$ONYX_LOG"
    while ! peridot_stop_native_services; do
        echo "ONYX_NATIVE_TAKEOVER_WAIT init_service_still_running" >>"$ONYX_LOG"
        sleep 2
    done
    echo "ONYX_NATIVE_TAKEOVER_READY init_service=stopped" >>"$ONYX_LOG"
else
    echo "ONYX_MODULE_MODE standalone state_dir=$ONYX_STATE_DIR" >>"$ONYX_LOG"
fi

# Start the daemon shipped by this module, never an old module or init-owned
# binary. Native init is stopped first so it cannot restart a competing daemon.
peridot_kill_existing_daemons

# Pin the daemon to the UID Android assigned to this exact APK. The daemon
# validates Unix peers with SO_PEERCRED and privileged-loopback peers against
# the kernel TCP table, so no app-to-root SELinux relaxation is required and no
# other ordinary application can issue hardware commands.
ONYX_UID_WAIT_LOGGED=0
while true; do
    ONYX_APP_UID="$(peridot_resolve_app_uid)"
    if [ -n "$ONYX_APP_UID" ]; then
        break
    fi
    if [ "$ONYX_UID_WAIT_LOGGED" -eq 0 ]; then
        echo "ONYX_MODULE_APP_UID_WAIT package=$ONYX_PACKAGE" >>"$ONYX_LOG"
        ONYX_UID_WAIT_LOGGED=1
    fi
    sleep 2
done
export ONYX_APP_UID
echo "ONYX_MODULE_APP_UID uid=$ONYX_APP_UID" >>"$ONYX_LOG"

while true; do
    if [ "$ONYX_NATIVE_MODE" -eq 1 ] && ! peridot_stop_native_services; then
        echo "ONYX_NATIVE_TAKEOVER_WAIT init_service_restarted" >>"$ONYX_LOG"
        sleep 2
        continue
    fi
    if [ -x "$ONYX_DAEMON" ] && ! pidof peridot_daemon >/dev/null 2>&1; then
        echo "ONYX_MODULE_DAEMON_START path=$ONYX_DAEMON" >>"$ONYX_LOG"
        "$ONYX_DAEMON" >>"$ONYX_LOG" 2>&1
        ONYX_EXIT=$?
        echo "ONYX_MODULE_DAEMON_EXIT code=$ONYX_EXIT" >>"$ONYX_LOG"
    fi
    sleep 2
done
