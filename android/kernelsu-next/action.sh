#!/system/bin/sh

MODDIR=${0%/*}
PERIDOT_CTL="$MODDIR/bin/peridot_ctl"

echo "Peridot Essential"
echo ""

if [ -f "$MODDIR/rom-native-mode" ]; then
    echo "Backend mode: ROM-native update layer"
    echo "Native init service: $(getprop init.svc.peridot_daemon 2>/dev/null)"
else
    echo "Backend mode: standalone root module"
fi

if /system/bin/pm path io.github.davvv98.peridotessential >/dev/null 2>&1; then
    echo "App: installed"
else
    echo "App: missing; flash the module again from your root manager"
fi

PERIDOT_PIDS="$(pidof peridot_daemon 2>/dev/null)"
if [ -z "$PERIDOT_PIDS" ]; then
    echo "Daemon: waiting for activation"
    echo "Install the module and reboot when you are ready."
    exit 0
fi

PERIDOT_PING="$($PERIDOT_CTL PING 2>&1)"
case "$PERIDOT_PING" in
    "OK PONG "*)
        echo "Daemon: healthy"
        echo "Daemon PID: $PERIDOT_PIDS"
        echo "Protocol: ${PERIDOT_PING#OK PONG }"
        PERIDOT_PRIMARY_PID="${PERIDOT_PIDS%% *}"
        PERIDOT_DAEMON_CONTEXT="$(cat "/proc/$PERIDOT_PRIMARY_PID/attr/current" 2>/dev/null)"
        [ -n "$PERIDOT_DAEMON_CONTEXT" ] && echo "Daemon SELinux: $PERIDOT_DAEMON_CONTEXT"
        ;;
    *)
        echo "Daemon: running but did not answer"
        echo "$PERIDOT_PING"
        exit 1
        ;;
esac

PERIDOT_SNAPSHOT="$($PERIDOT_CTL GET snapshot 2>/dev/null)"
PERIDOT_APP_CLIENT_SEEN="$(printf '%s\n' "$PERIDOT_SNAPSHOT" | tr ';' '\n' \
    | sed -n 's/^app_client_seen=//p' | head -n 1)"
PERIDOT_APP_CLIENT_TRANSPORT="$(printf '%s\n' "$PERIDOT_SNAPSHOT" | tr ';' '\n' \
    | sed -n 's/^app_client_transport=//p' | head -n 1)"
if [ "$PERIDOT_APP_CLIENT_SEEN" = "1" ]; then
    case "$PERIDOT_APP_CLIENT_TRANSPORT" in
        3) echo "App IPC: verified (authenticated loopback)" ;;
        2) echo "App IPC: verified (daemon-initiated)" ;;
        1) echo "App IPC: verified (direct policy)" ;;
        *) echo "App IPC: verified" ;;
    esac
else
    echo "App IPC: not verified yet; open Peridot Essential once"
fi

echo "$PERIDOT_SNAPSHOT" | tr ';' '\n' | grep -E '^(io|touch|touch_ack|touch_resampler_ready|perf)=' 2>/dev/null
