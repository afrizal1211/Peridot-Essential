#!/usr/bin/env bash
set -euo pipefail

PERIDOT_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PERIDOT_MODULE="$PERIDOT_PROJECT_ROOT/android/kernelsu-next"
PERIDOT_SERVICE="$PERIDOT_MODULE/service.sh"
PERIDOT_INSTALLER="$PERIDOT_MODULE/customize.sh"
PERIDOT_ACTION="$PERIDOT_MODULE/action.sh"

for PERIDOT_SCRIPT in customize.sh service.sh action.sh uninstall.sh; do
    bash -n "$PERIDOT_MODULE/$PERIDOT_SCRIPT"
    if LC_ALL=C grep -q $'\r' "$PERIDOT_MODULE/$PERIDOT_SCRIPT"; then
        echo "CRLF line endings are not allowed: $PERIDOT_SCRIPT" >&2
        exit 1
    fi
done

[ -f "$PERIDOT_MODULE/skip_mount" ] || {
    echo "Root module must remain script-only and skip system mounting" >&2
    exit 1
}

if find "$PERIDOT_MODULE" -maxdepth 1 -type f \
    \( -name 'sepolicy.rule' -o -name 'apply-sepolicy.sh' \) | grep -q .; then
    echo "Root module must not depend on app-to-root SELinux relaxation" >&2
    exit 1
fi

if grep -ERq 'magiskpolicy|ksud[[:space:]]+sepolicy|allow[[:space:]]+appdomain' \
    "$PERIDOT_MODULE"; then
    echo "Root module contains an obsolete SELinux patch path" >&2
    exit 1
fi

grep -Fxq 'export PERIDOT_REVERSE_IPC=1' "$PERIDOT_SERVICE"
grep -Fxq 'export PERIDOT_LOOPBACK_IPC=1' "$PERIDOT_SERVICE"
grep -Fq 'export PERIDOT_APP_UID' "$PERIDOT_SERVICE"
grep -Fq 'pm list packages -U' "$PERIDOT_SERVICE"
grep -Fq 'sys.boot_completed' "$PERIDOT_SERVICE"
grep -Fq 'PERIDOT_NATIVE_STATE_ROOT=/data/system/peridot-essential' "$PERIDOT_SERVICE"
grep -Fq 'PERIDOT_MODULE_MODE rom_native_update' "$PERIDOT_SERVICE"
grep -Fq 'ctl.stop' "$PERIDOT_SERVICE"
grep -Fq 'rom-native-mode' "$PERIDOT_INSTALLER"
grep -Fq 'update layer over the ROM-native installation' "$PERIDOT_INSTALLER"
grep -Fq 'uninstall-system-updates "$PERIDOT_PACKAGE"' "$PERIDOT_MODULE/uninstall.sh"
grep -Fq 'ctl.start' "$PERIDOT_MODULE/uninstall.sh"
grep -Fq 'App IPC: verified (daemon-initiated)' "$PERIDOT_ACTION"
grep -Fq 'App IPC: verified (authenticated loopback)' "$PERIDOT_ACTION"
grep -Fq 'INSTALL_FAILED_UPDATE_INCOMPATIBLE' "$PERIDOT_INSTALLER"
grep -Fq 'App/module version mismatch' "$PERIDOT_INSTALLER"
grep -Fq 'touch_resampler_ready' "$PERIDOT_ACTION"
[ -s "$PERIDOT_PROJECT_ROOT/runtime/daemon-rust/src/touch_resampler.rs" ] || {
    echo "The v1.18.0 touch resampler source is missing" >&2
    exit 1
}

grep -Fq 'PERIDOT_INSTALL_RESULT' "$PERIDOT_INSTALLER"
grep -Fq 'INSTALL_FAILED_UPDATE_INCOMPATIBLE' "$PERIDOT_INSTALLER"
grep -Fq '/system/bin/pm uninstall "$PERIDOT_PACKAGE"' "$PERIDOT_INSTALLER"
grep -Fq 'if [ "$PERIDOT_NATIVE_MODE" -eq 1 ]' "$PERIDOT_INSTALLER"
grep -Fq 'Application-local preferences were reset once' "$PERIDOT_INSTALLER"

if grep -Fq '/system/bin/pm uninstall -k' "$PERIDOT_INSTALLER"; then
    echo "Signature migration must not retain the obsolete package signing record" >&2
    exit 1
fi

if grep -Fq 'ROM-native Peridot Essential detected; do not install' "$PERIDOT_INSTALLER"; then
    echo "Installer still rejects ROM-native updates" >&2
    exit 1
fi

PERIDOT_MODULE_VERSION="$(sed -n 's/^version=v//p' "$PERIDOT_MODULE/module.prop")"
PERIDOT_MODULE_CODE="$(sed -n 's/^versionCode=//p' "$PERIDOT_MODULE/module.prop")"
PERIDOT_MANIFEST_VERSION="$(sed -n 's/.*android:versionName="\([^"]*\)".*/\1/p' \
    "$PERIDOT_PROJECT_ROOT/android/package/AndroidManifest.xml")"
PERIDOT_MANIFEST_CODE="$(sed -n 's/.*android:versionCode="\([^"]*\)".*/\1/p' \
    "$PERIDOT_PROJECT_ROOT/android/package/AndroidManifest.xml")"
PERIDOT_FLUTTER_VERSION="$(sed -n 's/^version: \([^+]*\)+.*/\1/p' \
    "$PERIDOT_PROJECT_ROOT/ui/flutter/pubspec.yaml")"
PERIDOT_FLUTTER_CODE="$(sed -n 's/^version: [^+]*+\([0-9]*\).*/\1/p' \
    "$PERIDOT_PROJECT_ROOT/ui/flutter/pubspec.yaml")"

for PERIDOT_VERSION in "$PERIDOT_MODULE_VERSION" "$PERIDOT_MANIFEST_VERSION" "$PERIDOT_FLUTTER_VERSION"; do
    [ "$PERIDOT_VERSION" = "$PERIDOT_MODULE_VERSION" ] || {
        echo "Application/module version mismatch" >&2
        exit 1
    }
done
for PERIDOT_CODE in "$PERIDOT_MODULE_CODE" "$PERIDOT_MANIFEST_CODE" "$PERIDOT_FLUTTER_CODE"; do
    [ "$PERIDOT_CODE" = "$PERIDOT_MODULE_CODE" ] || {
        echo "Application/module versionCode mismatch" >&2
        exit 1
    }
done

echo 'ROOT_MODULE_CONTRACT_TEST=PASS'
