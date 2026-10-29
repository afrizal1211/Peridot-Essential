#!/usr/bin/env bash
set -euo pipefail

PERIDOT_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PERIDOT_SERVICE_SOURCE="$PERIDOT_PROJECT_ROOT/android/kernelsu-next/service.sh"
PERIDOT_TEST_ROOT="$(mktemp -d)"
PERIDOT_TEST_RUNNER=""

peridot_test_cleanup() {
    if [ -n "$PERIDOT_TEST_RUNNER" ]; then
        kill "$PERIDOT_TEST_RUNNER" 2>/dev/null || true
        wait "$PERIDOT_TEST_RUNNER" 2>/dev/null || true
    fi
    rm -rf -- "$PERIDOT_TEST_ROOT"
}
trap peridot_test_cleanup EXIT

PERIDOT_TEST_MODULE="$PERIDOT_TEST_ROOT/nees_rodin_essential_backend"
PERIDOT_TEST_STATE="$PERIDOT_TEST_ROOT/state"
PERIDOT_TEST_BOOT_ID="$PERIDOT_TEST_ROOT/boot_id"
mkdir -p "$PERIDOT_TEST_MODULE" "$PERIDOT_TEST_STATE/watchdog.lock"

sed \
    -e "s|PERIDOT_ROOT=/data/adb/peridot-essential|PERIDOT_ROOT=$PERIDOT_TEST_STATE|" \
    -e "s|/proc/sys/kernel/random/boot_id|$PERIDOT_TEST_BOOT_ID|" \
    "$PERIDOT_SERVICE_SOURCE" >"$PERIDOT_TEST_MODULE/service.sh"
chmod 0755 "$PERIDOT_TEST_MODULE/service.sh"
printf '%s\n' 'current-test-boot' >"$PERIDOT_TEST_BOOT_ID"

# Reproduce the field failure: a stale lock from the previous boot contains
# the same numeric PID assigned to the newly launched service process.
(
    printf '%s\n' "$BASHPID" >"$PERIDOT_TEST_STATE/watchdog.lock/pid"
    printf '%s\n' 'previous-test-boot' >"$PERIDOT_TEST_STATE/watchdog.lock/boot_id"
    exec bash "$PERIDOT_TEST_MODULE/service.sh"
) &
PERIDOT_TEST_RUNNER=$!

PERIDOT_TEST_ATTEMPT=0
while [ "$PERIDOT_TEST_ATTEMPT" -lt 50 ]; do
    PERIDOT_TEST_ATTEMPT=$((PERIDOT_TEST_ATTEMPT + 1))
    PERIDOT_LOCK_PID="$(cat "$PERIDOT_TEST_STATE/watchdog.lock/pid" 2>/dev/null || true)"
    PERIDOT_LOCK_BOOT_ID="$(cat "$PERIDOT_TEST_STATE/watchdog.lock/boot_id" 2>/dev/null || true)"
    if [ "$PERIDOT_LOCK_PID" = "$PERIDOT_TEST_RUNNER" ] \
        && [ "$PERIDOT_LOCK_BOOT_ID" = 'current-test-boot' ]; then
        echo "WATCHDOG_PID_REUSE_TEST=PASS"
        exit 0
    fi
    sleep 0.02
done

echo "Watchdog did not reclaim a same-PID lock from the previous boot" >&2
exit 1
