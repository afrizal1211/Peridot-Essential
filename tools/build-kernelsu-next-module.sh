#!/usr/bin/env bash
set -euo pipefail

ONYX_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ONYX_MODULE_SOURCE="$ONYX_PROJECT_ROOT/android/kernelsu-next"
ONYX_SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
ONYX_STAMP="$(date +%Y%m%d-%H%M%S)"
ONYX_BUILD_ROOT="$ONYX_PROJECT_ROOT/out/kernelsu-next/$ONYX_STAMP"
ONYX_STAGE="$ONYX_BUILD_ROOT/module"
ONYX_APP_BUILD="$ONYX_BUILD_ROOT/application"

ONYX_VERSION="$(sed -n 's/^version=//p' "$ONYX_MODULE_SOURCE/module.prop")"
ONYX_ZIP_NAME="Peridot-Essential-KernelSU-Next-Magisk-${ONYX_VERSION}.zip"
ONYX_ZIP="$ONYX_BUILD_ROOT/$ONYX_ZIP_NAME"
ONYX_STABLE_ZIP="$ONYX_PROJECT_ROOT/out/Peridot-Essential-KernelSU-Next-Magisk.zip"

[ -n "${ONYX_KEYSTORE:-}" ] || {
    echo "ONYX_KEYSTORE is required for a distributable root module." >&2
    echo "Use one persistent signing key for every module update." >&2
    exit 1
}

ONYX_NDK_VERSION="${ONYX_NDK_VERSION:-$(find "$ONYX_SDK_ROOT/ndk" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V | tail -1)}"
ONYX_NDK="$ONYX_SDK_ROOT/ndk/$ONYX_NDK_VERSION"
ONYX_TOOLCHAIN="$ONYX_NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
ONYX_STRIP="$ONYX_TOOLCHAIN/llvm-strip"
ONYX_BUILD_TOOLS="${ONYX_BUILD_TOOLS_VERSION:-$(find "$ONYX_SDK_ROOT/build-tools" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V | tail -1)}"
ONYX_AAPT2="$ONYX_SDK_ROOT/build-tools/$ONYX_BUILD_TOOLS/aapt2"
ONYX_APKSIGNER="$ONYX_SDK_ROOT/build-tools/$ONYX_BUILD_TOOLS/apksigner"
ONYX_ZIPALIGN="$ONYX_SDK_ROOT/build-tools/$ONYX_BUILD_TOOLS/zipalign"

for ONYX_TOOL in "$ONYX_STRIP" "$ONYX_AAPT2" "$ONYX_APKSIGNER" "$ONYX_ZIPALIGN"; do
    [ -x "$ONYX_TOOL" ] || {
        echo "Missing Android build tool: $ONYX_TOOL" >&2
        exit 1
    }
done

"$ONYX_PROJECT_ROOT/tools/test-kernelsu-watchdog-lock.sh"
"$ONYX_PROJECT_ROOT/tools/test-root-module-contract.sh"
"$ONYX_PROJECT_ROOT/tools/test-release-sync.sh"

mkdir -p "$ONYX_STAGE/bin" "$ONYX_STAGE/app"

echo "===== ONYX ESSENTIAL — KERNELSU NEXT + MAGISK MODULE ====="
echo "version=$ONYX_VERSION"
echo "ndk=$ONYX_NDK_VERSION"
echo "output=$ONYX_ZIP"

ONYX_BUILD_ONLY=1 \
ONYX_OUTPUT_DIR="$ONYX_APP_BUILD" \
    "$ONYX_PROJECT_ROOT/build-and-install.sh"

ONYX_APK="$ONYX_APP_BUILD/Peridot-Essential.apk"
ONYX_DAEMON="$ONYX_APP_BUILD/host-cargo/aarch64-linux-android/release/peridot_daemon"
ONYX_CTL="$ONYX_APP_BUILD/host-cargo/aarch64-linux-android/release/peridot_ctl"

for ONYX_ARTIFACT in "$ONYX_APK" "$ONYX_DAEMON" "$ONYX_CTL"; do
    [ -f "$ONYX_ARTIFACT" ] || {
        echo "Missing release artifact: $ONYX_ARTIFACT" >&2
        exit 1
    }
done

install -m 0644 "$ONYX_MODULE_SOURCE/module.prop" "$ONYX_STAGE/module.prop"
install -m 0755 "$ONYX_MODULE_SOURCE/customize.sh" "$ONYX_STAGE/customize.sh"
install -m 0755 "$ONYX_MODULE_SOURCE/service.sh" "$ONYX_STAGE/service.sh"
install -m 0755 "$ONYX_MODULE_SOURCE/action.sh" "$ONYX_STAGE/action.sh"
install -m 0755 "$ONYX_MODULE_SOURCE/uninstall.sh" "$ONYX_STAGE/uninstall.sh"
install -m 0644 "$ONYX_MODULE_SOURCE/skip_mount" "$ONYX_STAGE/skip_mount"
install -m 0644 "$ONYX_APK" "$ONYX_STAGE/app/PeridotEssential.apk"
install -m 0755 "$ONYX_DAEMON" "$ONYX_STAGE/bin/peridot_daemon"
install -m 0755 "$ONYX_CTL" "$ONYX_STAGE/bin/peridot_ctl"

"$ONYX_STRIP" --strip-unneeded "$ONYX_STAGE/bin/peridot_daemon"
"$ONYX_STRIP" --strip-unneeded "$ONYX_STAGE/bin/peridot_ctl"

for ONYX_SCRIPT in customize.sh service.sh action.sh uninstall.sh; do
    bash -n "$ONYX_STAGE/$ONYX_SCRIPT"
    if LC_ALL=C grep -q $'\r' "$ONYX_STAGE/$ONYX_SCRIPT"; then
        echo "CRLF line endings are not allowed: $ONYX_SCRIPT" >&2
        exit 1
    fi
done

grep -Fxq 'export ONYX_REVERSE_IPC=1' "$ONYX_STAGE/service.sh" || {
    echo "Missing authenticated reverse IPC activation" >&2
    exit 1
}
grep -Fxq 'export ONYX_LOOPBACK_IPC=1' "$ONYX_STAGE/service.sh" || {
    echo "Missing authenticated loopback IPC activation" >&2
    exit 1
}
grep -Fq 'touch_resampler_ready' "$ONYX_STAGE/action.sh" || {
    echo "Missing v1.18.0 touch-resampler diagnostics" >&2
    exit 1
}
grep -Fq 'App IPC: verified (daemon-initiated)' "$ONYX_STAGE/action.sh" || {
    echo "Module Action does not verify the Android app transport" >&2
    exit 1
}
grep -Fq 'App IPC: verified (authenticated loopback)' "$ONYX_STAGE/action.sh" || {
    echo "Module Action does not report the authenticated loopback transport" >&2
    exit 1
}
grep -Fq 'ONYX_MODULE_MODE rom_native_update' "$ONYX_STAGE/service.sh" || {
    echo "Missing ROM-native daemon takeover path" >&2
    exit 1
}
grep -Fq 'uninstall-system-updates "$ONYX_PACKAGE"' "$ONYX_STAGE/uninstall.sh" || {
    echo "Missing ROM-native application rollback path" >&2
    exit 1
}

ONYX_MODID="$(sed -n 's/^id=//p' "$ONYX_STAGE/module.prop")"
ONYX_VERSION_CODE="$(sed -n 's/^versionCode=//p' "$ONYX_STAGE/module.prop")"
[[ "$ONYX_MODID" =~ ^[a-zA-Z][a-zA-Z0-9._-]+$ ]] || {
    echo "Invalid KernelSU module id: $ONYX_MODID" >&2
    exit 1
}
[[ "$ONYX_VERSION_CODE" =~ ^[0-9]+$ ]] || {
    echo "Invalid KernelSU versionCode: $ONYX_VERSION_CODE" >&2
    exit 1
}

ONYX_APK_BADGING="$("$ONYX_AAPT2" dump badging "$ONYX_STAGE/app/PeridotEssential.apk")"
ONYX_APK_HEADER="$(printf '%s\n' "$ONYX_APK_BADGING" | sed -n '1p')"
ONYX_APK_PACKAGE="$(printf '%s\n' "$ONYX_APK_HEADER" | sed -n "s/^package: name='\([^']*\)'.*/\1/p")"
ONYX_APK_VERSION_CODE="$(printf '%s\n' "$ONYX_APK_HEADER" | sed -n "s/^package: name='[^']*' versionCode='\([^']*\)'.*/\1/p")"
ONYX_APK_VERSION_NAME="$(printf '%s\n' "$ONYX_APK_HEADER" | sed -n "s/^package: name='[^']*' versionCode='[^']*' versionName='\([^']*\)'.*/\1/p")"

[ "$ONYX_APK_PACKAGE" = "io.github.davvv98.peridotessential" ] || {
    echo "Unexpected APK package: $ONYX_APK_PACKAGE" >&2
    exit 1
}
[ "$ONYX_APK_VERSION_CODE" = "$ONYX_VERSION_CODE" ] || {
    echo "APK/module versionCode mismatch: $ONYX_APK_VERSION_CODE / $ONYX_VERSION_CODE" >&2
    exit 1
}
[ "$ONYX_APK_VERSION_NAME" = "${ONYX_VERSION#v}" ] || {
    echo "APK/module version mismatch: $ONYX_APK_VERSION_NAME / $ONYX_VERSION" >&2
    exit 1
}

"$ONYX_APKSIGNER" verify --verbose "$ONYX_STAGE/app/PeridotEssential.apk" >/dev/null
ONYX_CERT_DIGEST="$("$ONYX_APKSIGNER" verify --print-certs \
    "$ONYX_STAGE/app/PeridotEssential.apk" \
    | sed -n 's/^.*certificate SHA-256 digest: //p' | head -n 1 \
    | tr '[:upper:]' '[:lower:]')"
# Forks may pin their own certificate; otherwise retain the repository default.
if [ -n "${ONYX_EXPECTED_CERT_SHA256:-}" ]; then
    ONYX_EXPECTED_CERT_DIGEST="$(printf '%s' "$ONYX_EXPECTED_CERT_SHA256" | tr -d '[:space:]:' | tr '[:upper:]' '[:lower:]')"
else
    ONYX_EXPECTED_CERT_DIGEST="$(tr -d '[:space:]:' <"$ONYX_PROJECT_ROOT/android/package/release-cert.sha256" | tr '[:upper:]' '[:lower:]')"
fi
[[ "$ONYX_EXPECTED_CERT_DIGEST" =~ ^[0-9a-f]{64}$ ]] || {
    echo "Invalid expected signing certificate SHA-256" >&2
    exit 1
}
[[ "$ONYX_CERT_DIGEST" =~ ^[0-9a-f]{64}$ ]] || {
    echo "Unable to read APK signing certificate" >&2
    exit 1
}
[ "$ONYX_CERT_DIGEST" = "$ONYX_EXPECTED_CERT_DIGEST" ] || {
    echo "Official release certificate mismatch" >&2
    echo "expected=$ONYX_EXPECTED_CERT_DIGEST" >&2
    echo "actual=$ONYX_CERT_DIGEST" >&2
    exit 1
}
"$ONYX_ZIPALIGN" -c -P 16 -v 4 "$ONYX_STAGE/app/PeridotEssential.apk" >/dev/null
ANDROID_SDK_ROOT="$ONYX_SDK_ROOT" bash "$ONYX_PROJECT_ROOT/tools/check-platform-dex.sh" "$ONYX_STAGE/app/PeridotEssential.apk"

file "$ONYX_STAGE/bin/peridot_daemon" | grep -q 'ARM aarch64'
file "$ONYX_STAGE/bin/peridot_ctl" | grep -q 'ARM aarch64'
readelf -lW "$ONYX_STAGE/bin/peridot_daemon" | grep -q '/system/bin/linker64'
readelf -lW "$ONYX_STAGE/bin/peridot_ctl" | grep -q '/system/bin/linker64'

for ONYX_BINARY in "$ONYX_STAGE/bin/peridot_daemon" "$ONYX_STAGE/bin/peridot_ctl"; do
    ONYX_LOAD_COUNT=0
    while read -r ONYX_ALIGNMENT; do
        [ -n "$ONYX_ALIGNMENT" ] || continue
        ONYX_LOAD_COUNT=$((ONYX_LOAD_COUNT + 1))
        if [ "$((ONYX_ALIGNMENT))" -lt 16384 ]; then
            echo "ELF alignment below 16 KB: $ONYX_BINARY ($ONYX_ALIGNMENT)" >&2
            exit 1
        fi
    done < <(readelf -lW "$ONYX_BINARY" | awk '$1 == "LOAD" { print $NF }')

    [ "$ONYX_LOAD_COUNT" -gt 0 ] || {
        echo "No ELF LOAD segments found: $ONYX_BINARY" >&2
        exit 1
    }
done

(
    cd "$ONYX_STAGE"
    zip -X -9 "$ONYX_ZIP" \
        module.prop customize.sh service.sh action.sh uninstall.sh skip_mount \
        app/PeridotEssential.apk bin/peridot_daemon bin/peridot_ctl >/dev/null
)

unzip -tq "$ONYX_ZIP"
ONYX_ENTRIES="$(unzip -Z1 "$ONYX_ZIP")"
for ONYX_REQUIRED in \
    module.prop customize.sh service.sh action.sh uninstall.sh skip_mount \
    app/PeridotEssential.apk bin/peridot_daemon bin/peridot_ctl; do
    echo "$ONYX_ENTRIES" | grep -Fxq "$ONYX_REQUIRED" || {
        echo "Missing module entry: $ONYX_REQUIRED" >&2
        exit 1
    }
done

if echo "$ONYX_ENTRIES" | grep -Eq '(^|/)system/'; then
    echo "Unexpected system overlay in the root module" >&2
    exit 1
fi
if echo "$ONYX_ENTRIES" | grep -Eq '(^|/)(sepolicy\.rule|apply-sepolicy\.sh)$'; then
    echo "Unexpected SELinux patch payload in the root module" >&2
    exit 1
fi

ONYX_STAGE_APK_SHA="$(sha256sum "$ONYX_STAGE/app/PeridotEssential.apk" | awk '{print $1}')"
ONYX_ZIP_APK_SHA="$(unzip -p "$ONYX_ZIP" app/PeridotEssential.apk | sha256sum | awk '{print $1}')"
[ "$ONYX_STAGE_APK_SHA" = "$ONYX_ZIP_APK_SHA" ] || {
    echo "APK payload checksum mismatch" >&2
    exit 1
}

cp -f "$ONYX_ZIP" "$ONYX_STABLE_ZIP"
ONYX_ZIP_SHA256="$(sha256sum "$ONYX_ZIP" | awk '{print $1}')"
printf '%s  %s\n' "$ONYX_ZIP_SHA256" "${ONYX_ZIP##*/}" \
    | tee "$ONYX_ZIP.sha256"

echo "ROOT_MODULE=PASS"
echo "MANAGERS=KernelSU Next, Magisk"
echo "APK_PACKAGE=$ONYX_APK_PACKAGE"
echo "APK_VERSION=$ONYX_APK_VERSION_NAME ($ONYX_APK_VERSION_CODE)"
echo "APK_CERT_SHA256=$ONYX_CERT_DIGEST"
echo "ZIP=$ONYX_ZIP"
echo "STABLE_ZIP=$ONYX_STABLE_ZIP"
