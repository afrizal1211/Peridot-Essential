#!/system/bin/sh

MODDIR=${0%/*}
PERIDOT_ROOT=/data/adb/peridot-essential
PERIDOT_NATIVE_STATE_ROOT=/data/system/peridot-essential
PERIDOT_DAEMON="$MODDIR/bin/peridot_daemon"
PERIDOT_LOG="$PERIDOT_ROOT/backend.log"
PERIDOT_LOG_OLD="$PERIDOT_ROOT/backend.log.1"
PERIDOT_WATCHDOG_PID="$PERIDOT_ROOT/watchdog.pid"
PERIDOT_WATCHDOG_LOCK="$PERIDOT_ROOT/watchdog.lock"
PERIDOT_WATCHDOG_BOOT_ID="$PERIDOT_WATCHDOG_LOCK/boot_id"
PERIDOT_SERVICE_PATH="$MODDIR/service.sh"
PERIDOT_CURRENT_BOOT_ID="$(cat /proc/sys/kernel/random/boot_id 2>/dev/null)"
PERIDOT_PACKAGE=io.github.davvv98.peridotessential

export PERIDOT_REVERSE_IPC=1
export PERIDOT_LOOPBACK_IPC=1

peridot_native_backend_present() {
    [ -f "$MODDIR/rom-native-mode" ] && return 0

    for PERIDOT_NATIVE_BINARY in \
        /product/bin/peridot_daemon \
        /system_ext/bin/peridot_daemon \
        /system/bin/peridot_daemon \
        /vendor/bin/peridot_daemon \
        /odm/bin/peridot_daemon; do
        [ -x "$PERIDOT_NATIVE_BINARY" ] && return 0
    done

    for PERIDOT_NATIVE_SERVICE in peridot_daemon peridot_essentiald; do
        [ -n "$(getprop "init.svc.$PERIDOT_NATIVE_SERVICE" 2>/dev/null)" ] && return 0
    done
    return 1
}

PERIDOT_NATIVE_MODE=0
if peridot_native_backend_present; then
    PERIDOT_NATIVE_MODE=1
    export PERIDOT_STATE_DIR="$PERIDOT_NATIVE_STATE_ROOT"
else
    export PERIDOT_STATE_DIR="$PERIDOT_ROOT"
fi

umask 077
mkdir -p "$PERIDOT_ROOT"
chmod 0700 "$PERIDOT_ROOT" 2>/dev/null
if [ "$PERIDOT_NATIVE_MODE" -eq 1 ]; then
    mkdir -p "$PERIDOT_NATIVE_STATE_ROOT"
    chmod 0700 "$PERIDOT_NATIVE_STATE_ROOT" 2>/dev/null
    /system/bin/restorecon -RF "$PERIDOT_NATIVE_STATE_ROOT" 2>/dev/null || \
        /system/bin/restorecon -R "$PERIDOT_NATIVE_STATE_ROOT" 2>/dev/null || true
fi

PERIDOT_LOG_BYTES=0
if [ -f "$PERIDOT_LOG" ]; then
    PERIDOT_LOG_BYTES="$(wc -c <"$PERIDOT_LOG" 2>/dev/null)"
fi
case "$PERIDOT_LOG_BYTES" in
    ''|*[!0-9]*) ;;
    *)
        if [ "$PERIDOT_LOG_BYTES" -gt 1048576 ]; then
            mv -f "$PERIDOT_LOG" "$PERIDOT_LOG_OLD"
        fi
        ;;
esac

# mkdir is an atomic early-boot lock and prevents duplicate watchdog loops.
if ! mkdir "$PERIDOT_WATCHDOG_LOCK" 2>/dev/null; then
    PERIDOT_EXISTING_PID="$(cat "$PERIDOT_WATCHDOG_LOCK/pid" 2>/dev/null)"
    PERIDOT_EXISTING_BOOT_ID="$(cat "$PERIDOT_WATCHDOG_BOOT_ID" 2>/dev/null)"
    PERIDOT_EXISTING_CMDLINE=""
    if [ -n "$PERIDOT_CURRENT_BOOT_ID" ] \
        && [ "$PERIDOT_EXISTING_BOOT_ID" = "$PERIDOT_CURRENT_BOOT_ID" ] \
        && [ "$PERIDOT_EXISTING_PID" != "$$" ] \
        && kill -0 "$PERIDOT_EXISTING_PID" 2>/dev/null; then
        PERIDOT_EXISTING_CMDLINE="$(tr '\000' ' ' <"/proc/$PERIDOT_EXISTING_PID/cmdline" 2>/dev/null)"
    fi
    case "$PERIDOT_EXISTING_CMDLINE" in
        *"$PERIDOT_SERVICE_PATH"*) exit 0 ;;
    esac

    rm -f "$PERIDOT_WATCHDOG_LOCK/pid" "$PERIDOT_WATCHDOG_BOOT_ID"
    rmdir "$PERIDOT_WATCHDOG_LOCK" 2>/dev/null || exit 0
    mkdir "$PERIDOT_WATCHDOG_LOCK" 2>/dev/null || exit 0
fi

printf '%s\n' "$$" >"$PERIDOT_WATCHDOG_PID"
printf '%s\n' "$$" >"$PERIDOT_WATCHDOG_LOCK/pid"
printf '%s\n' "$PERIDOT_CURRENT_BOOT_ID" >"$PERIDOT_WATCHDOG_BOOT_ID"

# Remove watchdogs left by older development packages of this same module.
for PERIDOT_PROC in /proc/[0-9]*; do
    PERIDOT_PID="${PERIDOT_PROC##*/}"
    [ "$PERIDOT_PID" = "$$" ] && continue
    PERIDOT_CMDLINE="$(tr '\000' ' ' <"$PERIDOT_PROC/cmdline" 2>/dev/null)"
    case "$PERIDOT_CMDLINE" in
        *"$PERIDOT_SERVICE_PATH"*)
            kill -9 "$PERIDOT_PID" 2>/dev/null
            ;;
    esac
done

peridot_cleanup() {
    PERIDOT_OWNER="$(cat "$PERIDOT_WATCHDOG_LOCK/pid" 2>/dev/null)"
    if [ "$PERIDOT_OWNER" = "$$" ]; then
        rm -f "$PERIDOT_WATCHDOG_PID" "$PERIDOT_WATCHDOG_LOCK/pid" "$PERIDOT_WATCHDOG_BOOT_ID"
        rmdir "$PERIDOT_WATCHDOG_LOCK" 2>/dev/null
    fi
}

trap peridot_cleanup EXIT
trap 'exit 0' HUP INT TERM

peridot_stop_native_services() {
    [ "$PERIDOT_NATIVE_MODE" -eq 1 ] || return 0

    for PERIDOT_NATIVE_SERVICE in peridot_daemon peridot_essentiald; do
        PERIDOT_NATIVE_STATE="$(getprop "init.svc.$PERIDOT_NATIVE_SERVICE" 2>/dev/null)"
        case "$PERIDOT_NATIVE_STATE" in
            ''|stopped) ;;
            *) /system/bin/setprop ctl.stop "$PERIDOT_NATIVE_SERVICE" 2>/dev/null || true ;;
        esac
    done

    PERIDOT_STOP_ATTEMPT=0
    while [ "$PERIDOT_STOP_ATTEMPT" -lt 10 ]; do
        PERIDOT_NATIVE_RUNNING=0
        for PERIDOT_NATIVE_SERVICE in peridot_daemon peridot_essentiald; do
            PERIDOT_NATIVE_STATE="$(getprop "init.svc.$PERIDOT_NATIVE_SERVICE" 2>/dev/null)"
            case "$PERIDOT_NATIVE_STATE" in
                starting|running|restarting|stopping) PERIDOT_NATIVE_RUNNING=1 ;;
            esac
        done
        [ "$PERIDOT_NATIVE_RUNNING" -eq 1 ] || return 0
        PERIDOT_STOP_ATTEMPT=$((PERIDOT_STOP_ATTEMPT + 1))
        sleep 1
    done
    return 1
}

peridot_kill_existing_daemons() {
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
}

peridot_resolve_app_uid() {
    PERIDOT_PACKAGE_ENTRY="$(/system/bin/pm list packages -U "$PERIDOT_PACKAGE" 2>/dev/null \
        | sed -n "s/^package:$PERIDOT_PACKAGE[[:space:]]*uid:\([0-9][0-9]*\).*$/\1/p" \
        | head -n 1)"
    case "$PERIDOT_PACKAGE_ENTRY" in
        ''|*[!0-9]*) ;;
        *)
            if [ "$PERIDOT_PACKAGE_ENTRY" -ge 10000 ] 2>/dev/null; then
                printf '%s\n' "$PERIDOT_PACKAGE_ENTRY"
                return 0
            fi
            ;;
    esac

    PERIDOT_PACKAGE_ENTRY="$(/system/bin/dumpsys package "$PERIDOT_PACKAGE" 2>/dev/null \
        | sed -n 's/.*userId=\([0-9][0-9]*\).*/\1/p' \
        | head -n 1)"
    case "$PERIDOT_PACKAGE_ENTRY" in
        ''|*[!0-9]*) return 1 ;;
        *)
            [ "$PERIDOT_PACKAGE_ENTRY" -ge 10000 ] 2>/dev/null || return 1
            printf '%s\n' "$PERIDOT_PACKAGE_ENTRY"
            ;;
    esac
}

# KernelSU/Magisk late-start scripts may run while framework/vendor services
# are still publishing their boot defaults. Start the backend only after the
# completed Android boot so every persisted Peridot setting wins that race.
while [ "$(getprop sys.boot_completed 2>/dev/null)" != "1" ]; do
    sleep 1
done

if [ "$PERIDOT_NATIVE_MODE" -eq 1 ]; then
    echo "PERIDOT_MODULE_MODE rom_native_update state_dir=$PERIDOT_STATE_DIR" >>"$PERIDOT_LOG"
    while ! peridot_stop_native_services; do
        echo "PERIDOT_NATIVE_TAKEOVER_WAIT init_service_still_running" >>"$PERIDOT_LOG"
        sleep 2
    done
    echo "PERIDOT_NATIVE_TAKEOVER_READY init_service=stopped" >>"$PERIDOT_LOG"
else
    echo "PERIDOT_MODULE_MODE standalone state_dir=$PERIDOT_STATE_DIR" >>"$PERIDOT_LOG"
fi

# Start the daemon shipped by this module, never an old module or init-owned
# binary. Native init is stopped first so it cannot restart a competing daemon.
peridot_kill_existing_daemons

# Pin the daemon to the UID Android assigned to this exact APK. The daemon
# validates Unix peers with SO_PEERCRED and privileged-loopback peers against
# the kernel TCP table, so no app-to-root SELinux relaxation is required and no
# other ordinary application can issue hardware commands.
PERIDOT_UID_WAIT_LOGGED=0
while true; do
    PERIDOT_APP_UID="$(peridot_resolve_app_uid)"
    if [ -n "$PERIDOT_APP_UID" ]; then
        break
    fi
    if [ "$PERIDOT_UID_WAIT_LOGGED" -eq 0 ]; then
        echo "PERIDOT_MODULE_APP_UID_WAIT package=$PERIDOT_PACKAGE" >>"$PERIDOT_LOG"
        PERIDOT_UID_WAIT_LOGGED=1
    fi
    sleep 2
done
export PERIDOT_APP_UID
echo "PERIDOT_MODULE_APP_UID uid=$PERIDOT_APP_UID" >>"$PERIDOT_LOG"

while true; do
    if [ "$PERIDOT_NATIVE_MODE" -eq 1 ] && ! peridot_stop_native_services; then
        echo "PERIDOT_NATIVE_TAKEOVER_WAIT init_service_restarted" >>"$PERIDOT_LOG"
        sleep 2
        continue
    fi
    if [ -x "$PERIDOT_DAEMON" ] && ! pidof peridot_daemon >/dev/null 2>&1; then
        echo "PERIDOT_MODULE_DAEMON_START path=$PERIDOT_DAEMON" >>"$PERIDOT_LOG"
        "$PERIDOT_DAEMON" >>"$PERIDOT_LOG" 2>&1
        PERIDOT_EXIT=$?
        echo "PERIDOT_MODULE_DAEMON_EXIT code=$PERIDOT_EXIT" >>"$PERIDOT_LOG"
    fi
    sleep 2
done
