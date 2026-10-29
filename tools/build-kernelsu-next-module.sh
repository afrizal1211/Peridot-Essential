#!/usr/bin/env bash
set -euo pipefail

PERIDOT_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PERIDOT_MODULE_SOURCE="$PERIDOT_PROJECT_ROOT/android/kernelsu-next"
PERIDOT_SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
PERIDOT_STAMP="$(date +%Y%m%d-%H%M%S)"
PERIDOT_BUILD_ROOT="$PERIDOT_PROJECT_ROOT/out/kernelsu-next/$PERIDOT_STAMP"
PERIDOT_STAGE="$PERIDOT_BUILD_ROOT/module"
PERIDOT_APP_BUILD="$PERIDOT_BUILD_ROOT/application"

PERIDOT_VERSION="$(sed -n 's/^version=//p' "$PERIDOT_MODULE_SOURCE/module.prop")"
PERIDOT_ZIP_NAME="Peridot-Essential-KernelSU-Next-Magisk-${PERIDOT_VERSION}.zip"
PERIDOT_ZIP="$PERIDOT_BUILD_ROOT/$PERIDOT_ZIP_NAME"
PERIDOT_STABLE_ZIP="$PERIDOT_PROJECT_ROOT/out/Peridot-Essential-KernelSU-Next-Magisk.zip"

[ -n "${PERIDOT_KEYSTORE:-}" ] || {
    echo "PERIDOT_KEYSTORE is required for a distributable root module." >&2
    echo "Use one persistent signing key for every module update." >&2
    exit 1
}

PERIDOT_NDK_VERSION="${PERIDOT_NDK_VERSION:-$(find "$PERIDOT_SDK_ROOT/ndk" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V | tail -1)}"
PERIDOT_NDK="$PERIDOT_SDK_ROOT/ndk/$PERIDOT_NDK_VERSION"
PERIDOT_TOOLCHAIN="$PERIDOT_NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
PERIDOT_STRIP="$PERIDOT_TOOLCHAIN/llvm-strip"
PERIDOT_BUILD_TOOLS="${PERIDOT_BUILD_TOOLS_VERSION:-$(find "$PERIDOT_SDK_ROOT/build-tools" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V | tail -1)}"
PERIDOT_AAPT2="$PERIDOT_SDK_ROOT/build-tools/$PERIDOT_BUILD_TOOLS/aapt2"
PERIDOT_APKSIGNER="$PERIDOT_SDK_ROOT/build-tools/$PERIDOT_BUILD_TOOLS/apksigner"
PERIDOT_ZIPALIGN="$PERIDOT_SDK_ROOT/build-tools/$PERIDOT_BUILD_TOOLS/zipalign"

for PERIDOT_TOOL in "$PERIDOT_STRIP" "$PERIDOT_AAPT2" "$PERIDOT_APKSIGNER" "$PERIDOT_ZIPALIGN"; do
    [ -x "$PERIDOT_TOOL" ] || {
        echo "Missing Android build tool: $PERIDOT_TOOL" >&2
        exit 1
    }
done

"$PERIDOT_PROJECT_ROOT/tools/test-kernelsu-watchdog-lock.sh"
"$PERIDOT_PROJECT_ROOT/tools/test-root-module-contract.sh"
"$PERIDOT_PROJECT_ROOT/tools/test-release-sync.sh"

mkdir -p "$PERIDOT_STAGE/bin" "$PERIDOT_STAGE/app"

echo "===== PERIDOT ESSENTIAL — KERNELSU NEXT + MAGISK MODULE ====="
echo "version=$PERIDOT_VERSION"
echo "ndk=$PERIDOT_NDK_VERSION"
echo "output=$PERIDOT_ZIP"

PERIDOT_BUILD_ONLY=1 \
PERIDOT_OUTPUT_DIR="$PERIDOT_APP_BUILD" \
    "$PERIDOT_PROJECT_ROOT/build-and-install.sh"

PERIDOT_APK="$PERIDOT_APP_BUILD/Peridot-Essential.apk"
PERIDOT_DAEMON="$PERIDOT_APP_BUILD/host-cargo/aarch64-linux-android/release/peridot_daemon"
PERIDOT_CTL="$PERIDOT_APP_BUILD/host-cargo/aarch64-linux-android/release/peridot_ctl"

for PERIDOT_ARTIFACT in "$PERIDOT_APK" "$PERIDOT_DAEMON" "$PERIDOT_CTL"; do
    [ -f "$PERIDOT_ARTIFACT" ] || {
        echo "Missing release artifact: $PERIDOT_ARTIFACT" >&2
        exit 1
    }
done

install -m 0644 "$PERIDOT_MODULE_SOURCE/module.prop" "$PERIDOT_STAGE/module.prop"
install -m 0755 "$PERIDOT_MODULE_SOURCE/customize.sh" "$PERIDOT_STAGE/customize.sh"
install -m 0755 "$PERIDOT_MODULE_SOURCE/service.sh" "$PERIDOT_STAGE/service.sh"
install -m 0755 "$PERIDOT_MODULE_SOURCE/action.sh" "$PERIDOT_STAGE/action.sh"
install -m 0755 "$PERIDOT_MODULE_SOURCE/uninstall.sh" "$PERIDOT_STAGE/uninstall.sh"
install -m 0644 "$PERIDOT_MODULE_SOURCE/skip_mount" "$PERIDOT_STAGE/skip_mount"
install -m 0644 "$PERIDOT_APK" "$PERIDOT_STAGE/app/PeridotEssential.apk"
install -m 0755 "$PERIDOT_DAEMON" "$PERIDOT_STAGE/bin/peridot_daemon"
install -m 0755 "$PERIDOT_CTL" "$PERIDOT_STAGE/bin/peridot_ctl"

"$PERIDOT_STRIP" --strip-unneeded "$PERIDOT_STAGE/bin/peridot_daemon"
"$PERIDOT_STRIP" --strip-unneeded "$PERIDOT_STAGE/bin/peridot_ctl"

for PERIDOT_SCRIPT in customize.sh service.sh action.sh uninstall.sh; do
    bash -n "$PERIDOT_STAGE/$PERIDOT_SCRIPT"
    if LC_ALL=C grep -q $'\r' "$PERIDOT_STAGE/$PERIDOT_SCRIPT"; then
        echo "CRLF line endings are not allowed: $PERIDOT_SCRIPT" >&2
        exit 1
    fi
done

grep -Fxq 'export PERIDOT_REVERSE_IPC=1' "$PERIDOT_STAGE/service.sh" || {
    echo "Missing authenticated reverse IPC activation" >&2
    exit 1
}
grep -Fxq 'export PERIDOT_LOOPBACK_IPC=1' "$PERIDOT_STAGE/service.sh" || {
    echo "Missing authenticated loopback IPC activation" >&2
    exit 1
}
grep -Fq 'touch_resampler_ready' "$PERIDOT_STAGE/action.sh" || {
    echo "Missing v1.18.0 touch-resampler diagnostics" >&2
    exit 1
}
grep -Fq 'App IPC: verified (daemon-initiated)' "$PERIDOT_STAGE/action.sh" || {
    echo "Module Action does not verify the Android app transport" >&2
    exit 1
}
grep -Fq 'App IPC: verified (authenticated loopback)' "$PERIDOT_STAGE/action.sh" || {
    echo "Module Action does not report the authenticated loopback transport" >&2
    exit 1
}
grep -Fq 'PERIDOT_MODULE_MODE rom_native_update' "$PERIDOT_STAGE/service.sh" || {
    echo "Missing ROM-native daemon takeover path" >&2
    exit 1
}
grep -Fq 'uninstall-system-updates "$PERIDOT_PACKAGE"' "$PERIDOT_STAGE/uninstall.sh" || {
    echo "Missing ROM-native application rollback path" >&2
    exit 1
}

PERIDOT_MODID="$(sed -n 's/^id=//p' "$PERIDOT_STAGE/module.prop")"
PERIDOT_VERSION_CODE="$(sed -n 's/^versionCode=//p' "$PERIDOT_STAGE/module.prop")"
[[ "$PERIDOT_MODID" =~ ^[a-zA-Z][a-zA-Z0-9._-]+$ ]] || {
    echo "Invalid KernelSU module id: $PERIDOT_MODID" >&2
    exit 1
}
[[ "$PERIDOT_VERSION_CODE" =~ ^[0-9]+$ ]] || {
    echo "Invalid KernelSU versionCode: $PERIDOT_VERSION_CODE" >&2
    exit 1
}

PERIDOT_APK_BADGING="$("$PERIDOT_AAPT2" dump badging "$PERIDOT_STAGE/app/PeridotEssential.apk")"
PERIDOT_APK_HEADER="$(printf '%s\n' "$PERIDOT_APK_BADGING" | sed -n '1p')"
PERIDOT_APK_PACKAGE="$(printf '%s\n' "$PERIDOT_APK_HEADER" | sed -n "s/^package: name='\([^']*\)'.*/\1/p")"
PERIDOT_APK_VERSION_CODE="$(printf '%s\n' "$PERIDOT_APK_HEADER" | sed -n "s/^package: name='[^']*' versionCode='\([^']*\)'.*/\1/p")"
PERIDOT_APK_VERSION_NAME="$(printf '%s\n' "$PERIDOT_APK_HEADER" | sed -n "s/^package: name='[^']*' versionCode='[^']*' versionName='\([^']*\)'.*/\1/p")"

[ "$PERIDOT_APK_PACKAGE" = "io.github.davvv98.peridotessential" ] || {
    echo "Unexpected APK package: $PERIDOT_APK_PACKAGE" >&2
    exit 1
}
[ "$PERIDOT_APK_VERSION_CODE" = "$PERIDOT_VERSION_CODE" ] || {
    echo "APK/module versionCode mismatch: $PERIDOT_APK_VERSION_CODE / $PERIDOT_VERSION_CODE" >&2
    exit 1
}
[ "$PERIDOT_APK_VERSION_NAME" = "${PERIDOT_VERSION#v}" ] || {
    echo "APK/module version mismatch: $PERIDOT_APK_VERSION_NAME / $PERIDOT_VERSION" >&2
    exit 1
}

"$PERIDOT_APKSIGNER" verify --verbose "$PERIDOT_STAGE/app/PeridotEssential.apk" >/dev/null
PERIDOT_CERT_DIGEST="$("$PERIDOT_APKSIGNER" verify --print-certs \
    "$PERIDOT_STAGE/app/PeridotEssential.apk" \
    | sed -n 's/^.*certificate SHA-256 digest: //p' | head -n 1 \
    | tr '[:upper:]' '[:lower:]')"
# Forks may pin their own certificate; otherwise retain the repository default.
if [ -n "${PERIDOT_EXPECTED_CERT_SHA256:-}" ]; then
    PERIDOT_EXPECTED_CERT_DIGEST="$(printf '%s' "$PERIDOT_EXPECTED_CERT_SHA256" | tr -d '[:space:]:' | tr '[:upper:]' '[:lower:]')"
else
    PERIDOT_EXPECTED_CERT_DIGEST="$(tr -d '[:space:]:' <"$PERIDOT_PROJECT_ROOT/android/package/release-cert.sha256" | tr '[:upper:]' '[:lower:]')"
fi
[[ "$PERIDOT_EXPECTED_CERT_DIGEST" =~ ^[0-9a-f]{64}$ ]] || {
    echo "Invalid expected signing certificate SHA-256" >&2
    exit 1
}
[[ "$PERIDOT_CERT_DIGEST" =~ ^[0-9a-f]{64}$ ]] || {
    echo "Unable to read APK signing certificate" >&2
    exit 1
}
[ "$PERIDOT_CERT_DIGEST" = "$PERIDOT_EXPECTED_CERT_DIGEST" ] || {
    echo "Official release certificate mismatch" >&2
    echo "expected=$PERIDOT_EXPECTED_CERT_DIGEST" >&2
    echo "actual=$PERIDOT_CERT_DIGEST" >&2
    exit 1
}
"$PERIDOT_ZIPALIGN" -c -P 16 -v 4 "$PERIDOT_STAGE/app/PeridotEssential.apk" >/dev/null
ANDROID_SDK_ROOT="$PERIDOT_SDK_ROOT" bash "$PERIDOT_PROJECT_ROOT/tools/check-platform-dex.sh" "$PERIDOT_STAGE/app/PeridotEssential.apk"

file "$PERIDOT_STAGE/bin/peridot_daemon" | grep -q 'ARM aarch64'
file "$PERIDOT_STAGE/bin/peridot_ctl" | grep -q 'ARM aarch64'
readelf -lW "$PERIDOT_STAGE/bin/peridot_daemon" | grep -q '/system/bin/linker64'
readelf -lW "$PERIDOT_STAGE/bin/peridot_ctl" | grep -q '/system/bin/linker64'

for PERIDOT_BINARY in "$PERIDOT_STAGE/bin/peridot_daemon" "$PERIDOT_STAGE/bin/peridot_ctl"; do
    PERIDOT_LOAD_COUNT=0
    while read -r PERIDOT_ALIGNMENT; do
        [ -n "$PERIDOT_ALIGNMENT" ] || continue
        PERIDOT_LOAD_COUNT=$((PERIDOT_LOAD_COUNT + 1))
        if [ "$((PERIDOT_ALIGNMENT))" -lt 16384 ]; then
            echo "ELF alignment below 16 KB: $PERIDOT_BINARY ($PERIDOT_ALIGNMENT)" >&2
            exit 1
        fi
    done < <(readelf -lW "$PERIDOT_BINARY" | awk '$1 == "LOAD" { print $NF }')

    [ "$PERIDOT_LOAD_COUNT" -gt 0 ] || {
        echo "No ELF LOAD segments found: $PERIDOT_BINARY" >&2
        exit 1
    }
done

(
    cd "$PERIDOT_STAGE"
    zip -X -9 "$PERIDOT_ZIP" \
        module.prop customize.sh service.sh action.sh uninstall.sh skip_mount \
        app/PeridotEssential.apk bin/peridot_daemon bin/peridot_ctl >/dev/null
)

unzip -tq "$PERIDOT_ZIP"
PERIDOT_ENTRIES="$(unzip -Z1 "$PERIDOT_ZIP")"
for PERIDOT_REQUIRED in \
    module.prop customize.sh service.sh action.sh uninstall.sh skip_mount \
    app/PeridotEssential.apk bin/peridot_daemon bin/peridot_ctl; do
    echo "$PERIDOT_ENTRIES" | grep -Fxq "$PERIDOT_REQUIRED" || {
        echo "Missing module entry: $PERIDOT_REQUIRED" >&2
        exit 1
    }
done

if echo "$PERIDOT_ENTRIES" | grep -Eq '(^|/)system/'; then
    echo "Unexpected system overlay in the root module" >&2
    exit 1
fi
if echo "$PERIDOT_ENTRIES" | grep -Eq '(^|/)(sepolicy\.rule|apply-sepolicy\.sh)$'; then
    echo "Unexpected SELinux patch payload in the root module" >&2
    exit 1
fi

PERIDOT_STAGE_APK_SHA="$(sha256sum "$PERIDOT_STAGE/app/PeridotEssential.apk" | awk '{print $1}')"
PERIDOT_ZIP_APK_SHA="$(unzip -p "$PERIDOT_ZIP" app/PeridotEssential.apk | sha256sum | awk '{print $1}')"
[ "$PERIDOT_STAGE_APK_SHA" = "$PERIDOT_ZIP_APK_SHA" ] || {
    echo "APK payload checksum mismatch" >&2
    exit 1
}

cp -f "$PERIDOT_ZIP" "$PERIDOT_STABLE_ZIP"
PERIDOT_ZIP_SHA256="$(sha256sum "$PERIDOT_ZIP" | awk '{print $1}')"
printf '%s  %s\n' "$PERIDOT_ZIP_SHA256" "${PERIDOT_ZIP##*/}" \
    | tee "$PERIDOT_ZIP.sha256"

echo "ROOT_MODULE=PASS"
echo "MANAGERS=KernelSU Next, Magisk"
echo "APK_PACKAGE=$PERIDOT_APK_PACKAGE"
echo "APK_VERSION=$PERIDOT_APK_VERSION_NAME ($PERIDOT_APK_VERSION_CODE)"
echo "APK_CERT_SHA256=$PERIDOT_CERT_DIGEST"
echo "ZIP=$PERIDOT_ZIP"
echo "STABLE_ZIP=$PERIDOT_STABLE_ZIP"
