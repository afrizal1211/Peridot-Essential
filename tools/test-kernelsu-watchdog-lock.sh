#!/usr/bin/env bash
set -euo pipefail

ONYX_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ONYX_SERVICE_SOURCE="$ONYX_PROJECT_ROOT/android/kernelsu-next/service.sh"
ONYX_TEST_ROOT="$(mktemp -d)"
ONYX_TEST_RUNNER=""

peridot_test_cleanup() {
    if [ -n "$ONYX_TEST_RUNNER" ]; then
        kill "$ONYX_TEST_RUNNER" 2>/dev/null || true
        wait "$ONYX_TEST_RUNNER" 2>/dev/null || true
    fi
    rm -rf -- "$ONYX_TEST_ROOT"
}
trap peridot_test_cleanup EXIT

ONYX_TEST_MODULE="$ONYX_TEST_ROOT/nees_rodin_essential_backend"
ONYX_TEST_STATE="$ONYX_TEST_ROOT/state"
ONYX_TEST_BOOT_ID="$ONYX_TEST_ROOT/boot_id"
mkdir -p "$ONYX_TEST_MODULE" "$ONYX_TEST_STATE/watchdog.lock"

sed \
    -e "s|ONYX_ROOT=/data/adb/peridot-essential|ONYX_ROOT=$ONYX_TEST_STATE|" \
    -e "s|/proc/sys/kernel/random/boot_id|$ONYX_TEST_BOOT_ID|" \
    "$ONYX_SERVICE_SOURCE" >"$ONYX_TEST_MODULE/service.sh"
chmod 0755 "$ONYX_TEST_MODULE/service.sh"
printf '%s\n' 'current-test-boot' >"$ONYX_TEST_BOOT_ID"

# Reproduce the field failure: a stale lock from the previous boot contains
# the same numeric PID assigned to the newly launched service process.
(
    printf '%s\n' "$BASHPID" >"$ONYX_TEST_STATE/watchdog.lock/pid"
    printf '%s\n' 'previous-test-boot' >"$ONYX_TEST_STATE/watchdog.lock/boot_id"
    exec bash "$ONYX_TEST_MODULE/service.sh"
) &
ONYX_TEST_RUNNER=$!

ONYX_TEST_ATTEMPT=0
while [ "$ONYX_TEST_ATTEMPT" -lt 50 ]; do
    ONYX_TEST_ATTEMPT=$((ONYX_TEST_ATTEMPT + 1))
    ONYX_LOCK_PID="$(cat "$ONYX_TEST_STATE/watchdog.lock/pid" 2>/dev/null || true)"
    ONYX_LOCK_BOOT_ID="$(cat "$ONYX_TEST_STATE/watchdog.lock/boot_id" 2>/dev/null || true)"
    if [ "$ONYX_LOCK_PID" = "$ONYX_TEST_RUNNER" ] \
        && [ "$ONYX_LOCK_BOOT_ID" = 'current-test-boot' ]; then
        echo "WATCHDOG_PID_REUSE_TEST=PASS"
        exit 0
    fi
    sleep 0.02
done

echo "Watchdog did not reclaim a same-PID lock from the previous boot" >&2
exit 1
