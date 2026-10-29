#!/usr/bin/env bash
set -euo pipefail

ONYX_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ONYX_MODULE="$ONYX_PROJECT_ROOT/android/kernelsu-next"
ONYX_SERVICE="$ONYX_MODULE/service.sh"
ONYX_INSTALLER="$ONYX_MODULE/customize.sh"
ONYX_ACTION="$ONYX_MODULE/action.sh"

for ONYX_SCRIPT in customize.sh service.sh action.sh uninstall.sh; do
    bash -n "$ONYX_MODULE/$ONYX_SCRIPT"
    if LC_ALL=C grep -q $'\r' "$ONYX_MODULE/$ONYX_SCRIPT"; then
        echo "CRLF line endings are not allowed: $ONYX_SCRIPT" >&2
        exit 1
    fi
done

[ -f "$ONYX_MODULE/skip_mount" ] || {
    echo "Root module must remain script-only and skip system mounting" >&2
    exit 1
}

if find "$ONYX_MODULE" -maxdepth 1 -type f \
    \( -name 'sepolicy.rule' -o -name 'apply-sepolicy.sh' \) | grep -q .; then
    echo "Root module must not depend on app-to-root SELinux relaxation" >&2
    exit 1
fi

if grep -ERq 'magiskpolicy|ksud[[:space:]]+sepolicy|allow[[:space:]]+appdomain' \
    "$ONYX_MODULE"; then
    echo "Root module contains an obsolete SELinux patch path" >&2
    exit 1
fi

grep -Fxq 'export ONYX_REVERSE_IPC=1' "$ONYX_SERVICE"
grep -Fxq 'export ONYX_LOOPBACK_IPC=1' "$ONYX_SERVICE"
grep -Fq 'export ONYX_APP_UID' "$ONYX_SERVICE"
grep -Fq 'pm list packages -U' "$ONYX_SERVICE"
grep -Fq 'sys.boot_completed' "$ONYX_SERVICE"
grep -Fq 'ONYX_NATIVE_STATE_ROOT=/data/system/peridot-essential' "$ONYX_SERVICE"
grep -Fq 'ONYX_MODULE_MODE rom_native_update' "$ONYX_SERVICE"
grep -Fq 'ctl.stop' "$ONYX_SERVICE"
grep -Fq 'rom-native-mode' "$ONYX_INSTALLER"
grep -Fq 'update layer over the ROM-native installation' "$ONYX_INSTALLER"
grep -Fq 'uninstall-system-updates "$ONYX_PACKAGE"' "$ONYX_MODULE/uninstall.sh"
grep -Fq 'ctl.start' "$ONYX_MODULE/uninstall.sh"
grep -Fq 'App IPC: verified (daemon-initiated)' "$ONYX_ACTION"
grep -Fq 'App IPC: verified (authenticated loopback)' "$ONYX_ACTION"
grep -Fq 'INSTALL_FAILED_UPDATE_INCOMPATIBLE' "$ONYX_INSTALLER"
grep -Fq 'App/module version mismatch' "$ONYX_INSTALLER"
grep -Fq 'touch_resampler_ready' "$ONYX_ACTION"
[ -s "$ONYX_PROJECT_ROOT/runtime/daemon-rust/src/touch_resampler.rs" ] || {
    echo "The v1.18.0 touch resampler source is missing" >&2
    exit 1
}

grep -Fq 'ONYX_INSTALL_RESULT' "$ONYX_INSTALLER"
grep -Fq 'INSTALL_FAILED_UPDATE_INCOMPATIBLE' "$ONYX_INSTALLER"
grep -Fq '/system/bin/pm uninstall "$ONYX_PACKAGE"' "$ONYX_INSTALLER"
grep -Fq 'if [ "$ONYX_NATIVE_MODE" -eq 1 ]' "$ONYX_INSTALLER"
grep -Fq 'Application-local preferences were reset once' "$ONYX_INSTALLER"

if grep -Fq '/system/bin/pm uninstall -k' "$ONYX_INSTALLER"; then
    echo "Signature migration must not retain the obsolete package signing record" >&2
    exit 1
fi

if grep -Fq 'ROM-native Peridot Essential detected; do not install' "$ONYX_INSTALLER"; then
    echo "Installer still rejects ROM-native updates" >&2
    exit 1
fi

ONYX_MODULE_VERSION="$(sed -n 's/^version=v//p' "$ONYX_MODULE/module.prop")"
ONYX_MODULE_CODE="$(sed -n 's/^versionCode=//p' "$ONYX_MODULE/module.prop")"
ONYX_MANIFEST_VERSION="$(sed -n 's/.*android:versionName="\([^"]*\)".*/\1/p' \
    "$ONYX_PROJECT_ROOT/android/package/AndroidManifest.xml")"
ONYX_MANIFEST_CODE="$(sed -n 's/.*android:versionCode="\([^"]*\)".*/\1/p' \
    "$ONYX_PROJECT_ROOT/android/package/AndroidManifest.xml")"
ONYX_FLUTTER_VERSION="$(sed -n 's/^version: \([^+]*\)+.*/\1/p' \
    "$ONYX_PROJECT_ROOT/ui/flutter/pubspec.yaml")"
ONYX_FLUTTER_CODE="$(sed -n 's/^version: [^+]*+\([0-9]*\).*/\1/p' \
    "$ONYX_PROJECT_ROOT/ui/flutter/pubspec.yaml")"

for ONYX_VERSION in "$ONYX_MODULE_VERSION" "$ONYX_MANIFEST_VERSION" "$ONYX_FLUTTER_VERSION"; do
    [ "$ONYX_VERSION" = "$ONYX_MODULE_VERSION" ] || {
        echo "Application/module version mismatch" >&2
        exit 1
    }
done
for ONYX_CODE in "$ONYX_MODULE_CODE" "$ONYX_MANIFEST_CODE" "$ONYX_FLUTTER_CODE"; do
    [ "$ONYX_CODE" = "$ONYX_MODULE_CODE" ] || {
        echo "Application/module versionCode mismatch" >&2
        exit 1
    }
done

echo 'ROOT_MODULE_CONTRACT_TEST=PASS'
