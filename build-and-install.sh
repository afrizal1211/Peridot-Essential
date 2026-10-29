#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
UI="$ROOT/ui/flutter"
MAIN="$UI/lib/main.dart"
BACKEND_DART="$UI/lib/backend/peridot_backend.dart"
HOST_LIB="$ROOT/runtime/host-rust/src/lib.rs"
HOST_BRIDGE="$ROOT/runtime/host-rust/src/backend_bridge.rs"
DAEMON_LIB="$ROOT/runtime/daemon-rust/src/lib.rs"

bash "$ROOT/tools/cleanup-legacy-paths.sh"

SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
PKG="io.github.davvv98.peridotessential"
COMP="$PKG/.PeridotActivity"

STAMP="${ONYX_BUILD_STAMP:-$(date +%Y%m%d-%H%M%S)}"
OUT="${ONYX_OUTPUT_DIR:-$ROOT/out/release/$STAMP}"
AOT="$OUT/flutter-aot"
APK="$OUT/Peridot-Essential.apk"
LOG="$OUT/device-logcat.txt"

mkdir -p "$OUT" "$AOT"

echo "===== ONYX ESSENTIAL — NATIVE RELEASE BUILD ====="
echo "root=$ROOT"
echo "output=$OUT"
echo "apk=$APK"

echo
echo "===== 1. FLUTTER ANALYZE ====="
(
    cd "$UI"
    flutter analyze
)

echo
echo "===== 2. BUILD FLUTTER AOT RELEASE BUNDLE ====="
(
    cd "$UI"
    flutter assemble \
        --output="$AOT" \
        -dTargetPlatform=android-arm64 \
        -dBuildMode=release \
        -dTargetFile=lib/main.dart \
        -dTrackWidgetCreation=false \
        android_aot_bundle_release_android-arm64
)

echo
echo "===== 3. DETECT TOOLCHAINS ====="
BUILD_TOOLS="${ONYX_BUILD_TOOLS_VERSION:-$(find "$SDK/build-tools" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V | tail -1)}"
PLATFORM="${ONYX_PLATFORM_VERSION:-$(find "$SDK/platforms" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sed -nE 's/^android-([0-9]+)$/\1/p' | sort -n | tail -1)}"
NDK_VER="${ONYX_NDK_VERSION:-$(find "$SDK/ndk" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V | tail -1)}"

NDK="$SDK/ndk/$NDK_VER"
LINKER="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android31-clang"
AAPT2="$SDK/build-tools/$BUILD_TOOLS/aapt2"
ZIPALIGN="$SDK/build-tools/$BUILD_TOOLS/zipalign"
APKSIGNER="$SDK/build-tools/$BUILD_TOOLS/apksigner"
ANDROID_JAR="$SDK/platforms/android-$PLATFORM/android.jar"
ENGINE_PREBUILT="$ROOT/runtime/flutter-engine/prebuilt/android-arm64"
ICU="$ENGINE_PREBUILT/icudtl.dat"

for ONYX_TOOL in flutter cargo python3 keytool readelf sha256sum unzip zip; do
    command -v "$ONYX_TOOL" >/dev/null 2>&1 || {
        echo "Missing required tool: $ONYX_TOOL" >&2
        exit 1
    }
done

for ONYX_FILE in "$LINKER" "$AAPT2" "$ZIPALIGN" "$APKSIGNER" "$ANDROID_JAR" \
    "$ENGINE_PREBUILT/libflutter_engine.so" "$ICU"; do
    [ -f "$ONYX_FILE" ] || {
        echo "Missing build dependency: $ONYX_FILE" >&2
        exit 1
    }
done

echo "BUILD_TOOLS=$BUILD_TOOLS"
echo "PLATFORM=android-$PLATFORM"
echo "NDK=$NDK_VER"

echo
echo "===== 4. BUILD HOST & DAEMON RUST RUNTIME ====="
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LINKER"
export CARGO_TARGET_DIR="$OUT/host-cargo"
export RUSTFLAGS="-L native=$ROOT/runtime/flutter-engine/prebuilt/android-arm64 -C link-arg=-Wl,-z,max-page-size=16384"

(
    cd "$ROOT/runtime/host-rust"
    cargo build --release --target aarch64-linux-android
)

(
    cd "$ROOT/runtime/daemon-rust"
    cargo build --release --target aarch64-linux-android
)

HOST_SO="$CARGO_TARGET_DIR/aarch64-linux-android/release/libperidot_essential_host.so"
DAEMON_BIN="$CARGO_TARGET_DIR/aarch64-linux-android/release/peridot_daemon"
CTL_BIN="$CARGO_TARGET_DIR/aarch64-linux-android/release/peridot_ctl"

echo
echo "===== 5. PACKAGE ZERO-DEX / 16K APK ====="
STAGE="$OUT/stage"
ASSETS="$STAGE/assets"
LIBS="$STAGE/lib/arm64-v8a"
UNALIGNED="$OUT/unaligned.apk"
ALIGNED="$OUT/aligned.apk"

mkdir -p "$ASSETS/flutter_assets" "$LIBS"

cp -a "$AOT/flutter_assets/." "$ASSETS/flutter_assets/"
find "$AOT/flutter_assets" -type f -printf '%P\n' | LC_ALL=C sort > "$ASSETS/flutter_assets.index"
cp -a "$ICU" "$ASSETS/icudtl.dat"

ONYX_RUNTIME_ASSET_STAMP="$(
    (
        cd "$ASSETS"
        find flutter_assets -type f -print0 \
            | LC_ALL=C sort -z \
            | xargs -0 sha256sum
        sha256sum icudtl.dat
    ) | sha256sum | awk '{print $1}'
)"
printf '%s\n' "$ONYX_RUNTIME_ASSET_STAMP" > "$ASSETS/peridot_runtime.stamp"
echo "RUNTIME_ASSET_STAMP=$ONYX_RUNTIME_ASSET_STAMP"

cp -a "$HOST_SO" "$LIBS/libperidot_essential_host.so"
cp -a "$ENGINE_PREBUILT/libflutter_engine.so" "$LIBS/libflutter_engine.so"
cp -a "$AOT/arm64-v8a/app.so" "$LIBS/libapp.so"

# The embedder target may contain hundreds of MB of debug symbols. Keep its
# dynamic exports but remove debug/static symbols before adding it to the APK.
LLVM_STRIP="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"
[ -x "$LLVM_STRIP" ] || { echo "Missing LLVM strip: $LLVM_STRIP" >&2; exit 1; }
ENGINE_SIZE_BEFORE="$(stat -c %s "$LIBS/libflutter_engine.so")"
"$LLVM_STRIP" --strip-unneeded "$LIBS/libflutter_engine.so"
ENGINE_SIZE_AFTER="$(stat -c %s "$LIBS/libflutter_engine.so")"
echo "FLUTTER_ENGINE_SIZE before=$ENGINE_SIZE_BEFORE after=$ENGINE_SIZE_AFTER"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf 'Flutter engine: %s -> %s bytes after stripping.\n' "$ENGINE_SIZE_BEFORE" "$ENGINE_SIZE_AFTER" >> "$GITHUB_STEP_SUMMARY"
fi


"$AAPT2" compile --dir "$ROOT/android/package/res" -o "$STAGE/compiled_res.zip"

"$AAPT2" link \
    -o "$UNALIGNED" \
    -I "$ANDROID_JAR" \
    --manifest "$ROOT/android/package/AndroidManifest.xml" \
    -A "$ASSETS" \
    -R "$STAGE/compiled_res.zip" \
    --auto-add-overlay

python3 - "$UNALIGNED" "$LIBS" <<'PY'
from pathlib import Path
from zipfile import ZIP_STORED, ZipFile
import sys

apk = Path(sys.argv[1])
libs = Path(sys.argv[2])

with ZipFile(apk, "a", compression=ZIP_STORED, allowZip64=True) as z:
    for path in sorted(libs.glob("*.so")):
        z.write(path, f"lib/arm64-v8a/{path.name}", compress_type=ZIP_STORED)
PY

ANDROID_SDK_ROOT="$SDK" bash "$ROOT/tools/compile-platform-bridge.sh" "$STAGE"
(cd "$STAGE/platform-dex" && zip -0 "$UNALIGNED" classes.dex >/dev/null)
"$ZIPALIGN" -P 16 -f 4 "$UNALIGNED" "$ALIGNED"

KS_PASS="${ONYX_KEYSTORE_PASS:-android}"
KEY_PASS="${ONYX_KEY_PASS:-$KS_PASS}"
KEYSTORE="${ONYX_KEYSTORE:-}"

if [ -z "$KEYSTORE" ]; then
    KEYSTORE="$ROOT/out/signing/peridot-essential-development.jks"
    ALIAS="peridot-essential-development"
    mkdir -p "$(dirname "$KEYSTORE")"
    if [ ! -f "$KEYSTORE" ]; then
        keytool -genkeypair \
            -keystore "$KEYSTORE" \
            -storetype JKS \
            -storepass "$KS_PASS" \
            -keypass "$KEY_PASS" \
            -alias "$ALIAS" \
            -keyalg RSA \
            -keysize 4096 \
            -validity 10000 \
            -dname "CN=Peridot Essential Development,O=Peridot Essential,C=NP" \
            >/dev/null 2>&1
    fi
else
    [ -f "$KEYSTORE" ] || {
        echo "ONYX_KEYSTORE does not exist: $KEYSTORE" >&2
        exit 1
    }
    ALIAS="${ONYX_KEY_ALIAS:-$(keytool -list -keystore "$KEYSTORE" -storepass "$KS_PASS" | sed -nE 's/^([^,]+), .*/\1/p' | head -1)}"
fi

[ -n "$ALIAS" ] || {
    echo "Unable to determine the APK signing alias" >&2
    exit 1
}

cp -a "$ALIGNED" "$APK"

"$APKSIGNER" sign \
    --ks "$KEYSTORE" \
    --ks-key-alias "$ALIAS" \
    --ks-pass "pass:$KS_PASS" \
    --key-pass "pass:$KEY_PASS" \
    "$APK"

"$APKSIGNER" verify --verbose "$APK" >/dev/null
echo "APK_SIGNATURE=PASS"

echo
echo "===== 6. VERIFY PLATFORM BRIDGE AND 16K ALIGNMENT ====="
unzip -Z1 "$APK" | sort > "$OUT/apk-files.txt"
ANDROID_SDK_ROOT="$SDK" bash "$ROOT/tools/check-platform-dex.sh" "$APK"

"$ZIPALIGN" -c -P 16 -v 4 "$APK" >/dev/null
echo "16K_ZIPALIGN=PASS"

verify_elf_alignment() {
    local ONYX_ELF="$1"
    local ONYX_ALIGNMENT
    local ONYX_LOADS=0

    while read -r ONYX_ALIGNMENT; do
        [ -n "$ONYX_ALIGNMENT" ] || continue
        ONYX_LOADS=$((ONYX_LOADS + 1))
        if [ "$((ONYX_ALIGNMENT))" -lt 16384 ]; then
            echo "16K_ELF_ALIGNMENT=FAIL file=$ONYX_ELF align=$ONYX_ALIGNMENT" >&2
            exit 1
        fi
    done < <(readelf -lW "$ONYX_ELF" | awk '$1 == "LOAD" { print $NF }')

    [ "$ONYX_LOADS" -gt 0 ] || {
        echo "No ELF LOAD segments found: $ONYX_ELF" >&2
        exit 1
    }
}

for ONYX_ELF in \
    "$LIBS/libperidot_essential_host.so" \
    "$LIBS/libflutter_engine.so" \
    "$LIBS/libapp.so" \
    "$DAEMON_BIN" \
    "$CTL_BIN"; do
    verify_elf_alignment "$ONYX_ELF"
done
echo "16K_ELF_ALIGNMENT=PASS"

python3 - "$HOST_SO" "$BACKEND_DART" <<'PY'
import re
import subprocess
import sys

host, dart = sys.argv[1:]
source = open(dart, encoding="utf-8").read()
required = set(
    re.findall(r"'((?:peridot_backend|peridot_host)_[A-Za-z0-9_]+)'", source)
)
symbols = subprocess.check_output(["readelf", "-Ws", host], text=True)
exported = set(
    re.findall(
        r"\b((?:peridot_backend|peridot_host)_[A-Za-z0-9_]+)\s*$",
        symbols,
        re.MULTILINE,
    )
)
missing = sorted(required - exported)
if missing:
    raise SystemExit("Missing native API symbols: " + ", ".join(missing))
print(f"NATIVE_API_PARITY=PASS ({len(required)} Dart lookups)")
PY

if [ "${ONYX_BUILD_ONLY:-0}" = "1" ]; then
    echo
    echo "===== BUILD-ONLY SUCCESSFUL! ====="
    echo "APK=$APK"
    echo "DAEMON=$DAEMON_BIN"
    echo "CTL=$CTL_BIN"
    exit 0
fi

echo
echo "===== 7. INSTALL ON CONNECTED PHONE ====="
adb install --no-incremental -r "$APK" || adb install -r "$APK"
if [ -f "$DAEMON_BIN" ]; then
    echo "Updating daemon and ctl binaries on device..."
    adb push "$DAEMON_BIN" /data/local/tmp/peridot_daemon >/dev/null
    adb push "$CTL_BIN" /data/local/tmp/peridot_ctl >/dev/null
    adb push "$ROOT/android/kernelsu-next/service.sh" /data/local/tmp/peridot-service.sh >/dev/null
    adb push "$ROOT/android/kernelsu-next/module.prop" /data/local/tmp/peridot-module.prop >/dev/null
    adb shell "su -c 'if [ -f /data/adb/peridot-essential/watchdog.pid ]; then kill \$(cat /data/adb/peridot-essential/watchdog.pid) 2>/dev/null || true; fi; for p in \$(pidof peridot_daemon) \$(pidof peridot_essentiald); do kill -9 \$p 2>/dev/null || true; done; sleep 0.4; mkdir -p /data/adb/peridot-essential /data/adb/modules/nees_rodin_essential_backend/bin; cp -f /data/local/tmp/peridot_daemon /data/adb/modules/nees_rodin_essential_backend/bin/peridot_daemon; cp -f /data/local/tmp/peridot_ctl /data/adb/modules/nees_rodin_essential_backend/bin/peridot_ctl; cp -f /data/local/tmp/peridot-service.sh /data/adb/modules/nees_rodin_essential_backend/service.sh; cp -f /data/local/tmp/peridot-module.prop /data/adb/modules/nees_rodin_essential_backend/module.prop; touch /data/adb/modules/nees_rodin_essential_backend/skip_mount; chmod 0755 /data/adb/modules/nees_rodin_essential_backend/bin/peridot_daemon /data/adb/modules/nees_rodin_essential_backend/bin/peridot_ctl /data/adb/modules/nees_rodin_essential_backend/service.sh; chmod 0644 /data/adb/modules/nees_rodin_essential_backend/module.prop /data/adb/modules/nees_rodin_essential_backend/skip_mount; chown -R root:root /data/adb/peridot-essential /data/adb/modules/nees_rodin_essential_backend; nohup /data/adb/modules/nees_rodin_essential_backend/service.sh >/dev/null 2>&1 &'" >/dev/null
    sleep 1
fi

echo
echo "===== 8. LAUNCH APP ====="
adb shell am force-stop "$PKG" || true
adb logcat -c || true
adb shell am start -W -n "$COMP"

sleep 3

adb logcat -d -v threadtime > "$LOG" 2>/dev/null || true
adb exec-out screencap -p > "$OUT/screenshot-home.png" 2>/dev/null || true

PID="$(adb shell pidof "$PKG" 2>/dev/null | tr -d '\r' | awk '{print $1}')"
echo "RUNNING_PID=$PID"

echo
echo "===== BUILD & INSTALL SUCCESSFUL! ====="
echo "APK=$APK"
echo "SCREENSHOT=$OUT/screenshot-home.png"
