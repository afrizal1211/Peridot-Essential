#!/usr/bin/env bash
set -euo pipefail

ONYX_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ONYX_STAMP="$(date +%Y%m%d-%H%M%S)"
ONYX_DESTINATION="${1:-$ONYX_ROOT/dist/aosp/PeridotEssential-$ONYX_STAMP}"
ONYX_BUILD_DIR="$ONYX_ROOT/out/aosp-export/$ONYX_STAMP/build"
ONYX_TEMPLATE="$ONYX_ROOT/android/aosp"
ONYX_SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
ONYX_NDK_VERSION="$(find "$ONYX_SDK_ROOT/ndk" -mindepth 1 -maxdepth 1 \
    -type d -printf '%f\n' | sort -V | tail -1)"
ONYX_STRIP="$ONYX_SDK_ROOT/ndk/$ONYX_NDK_VERSION/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"

[ -n "${ONYX_KEYSTORE:-}" ] || {
    echo "ONYX_KEYSTORE is required for a reproducibly signed AOSP bundle." >&2
    echo "ROM maintainers must keep the same private key for every update." >&2
    exit 1
}
[ -f "$ONYX_KEYSTORE" ] || {
    echo "ONYX_KEYSTORE does not exist: $ONYX_KEYSTORE" >&2
    exit 1
}

for ONYX_TOOL in openssl readelf sha256sum unzip; do
    command -v "$ONYX_TOOL" >/dev/null 2>&1 || {
        echo "Missing AOSP export tool: $ONYX_TOOL" >&2
        exit 1
    }
done
[ -x "$ONYX_STRIP" ] || {
    echo "Missing Android LLVM strip tool: $ONYX_STRIP" >&2
    exit 1
}

"$ONYX_ROOT/tools/test-aosp-integration.sh"
"$ONYX_ROOT/tools/test-release-sync.sh"

if [ -e "$ONYX_DESTINATION" ]; then
    echo "Destination already exists: $ONYX_DESTINATION" >&2
    exit 1
fi

ONYX_BUILD_ONLY=1 \
ONYX_OUTPUT_DIR="$ONYX_BUILD_DIR" \
    "$ONYX_ROOT/build-and-install.sh"

ONYX_APK="$ONYX_BUILD_DIR/Peridot-Essential.apk"
ONYX_DAEMON="$ONYX_BUILD_DIR/host-cargo/aarch64-linux-android/release/peridot_daemon"
ONYX_CTL="$ONYX_BUILD_DIR/host-cargo/aarch64-linux-android/release/peridot_ctl"
ONYX_BUILD_TOOLS="$(find "$ONYX_SDK_ROOT/build-tools" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V | tail -1)"
ONYX_APKSIGNER="$ONYX_SDK_ROOT/build-tools/$ONYX_BUILD_TOOLS/apksigner"

[ -x "$ONYX_APKSIGNER" ] || {
    echo "Missing Android APK signer: $ONYX_APKSIGNER" >&2
    exit 1
}

for ONYX_ARTIFACT in "$ONYX_APK" "$ONYX_DAEMON" "$ONYX_CTL"; do
    [ -f "$ONYX_ARTIFACT" ] || {
        echo "Missing AOSP artifact: $ONYX_ARTIFACT" >&2
        exit 1
    }
done

mkdir -p "$ONYX_DESTINATION/prebuilt" "$ONYX_DESTINATION/docs"
cp -a "$ONYX_TEMPLATE/Android.bp" "$ONYX_DESTINATION/Android.bp"
cp -a "$ONYX_TEMPLATE/peridot-essential.mk" "$ONYX_DESTINATION/peridot-essential.mk"
cp -a "$ONYX_TEMPLATE/BoardConfigPeridotEssential.mk" "$ONYX_DESTINATION/BoardConfigPeridotEssential.mk"
cp -a "$ONYX_TEMPLATE/peridot_daemon.rc" "$ONYX_DESTINATION/peridot_daemon.rc"
cp -a "$ONYX_TEMPLATE/sepolicy" "$ONYX_DESTINATION/sepolicy"
cp -a "$ONYX_TEMPLATE/README.md" "$ONYX_DESTINATION/README.md"
cp -a "$ONYX_ROOT/docs/ROM_INTEGRATION.md" "$ONYX_DESTINATION/docs/ROM_INTEGRATION.md"
cp -a "$ONYX_ROOT/docs/UNPACKED_ROM_INTEGRATION.md" "$ONYX_DESTINATION/docs/UNPACKED_ROM_INTEGRATION.md"
cp -a "$ONYX_ROOT/docs/UNPACKED_ROM_PORTER_GUIDE.md" "$ONYX_DESTINATION/docs/UNPACKED_ROM_PORTER_GUIDE.md"
mkdir -p "$ONYX_DESTINATION/tools"
cp -a "$ONYX_ROOT/tools/prepare-aosp-touch-policy.py" "$ONYX_DESTINATION/tools/prepare-aosp-touch-policy.py"
install -m 0644 "$ONYX_APK" "$ONYX_DESTINATION/prebuilt/PeridotEssential.apk"
install -m 0755 "$ONYX_DAEMON" "$ONYX_DESTINATION/prebuilt/peridot_daemon"
install -m 0755 "$ONYX_CTL" "$ONYX_DESTINATION/prebuilt/peridot_ctl"
"$ONYX_STRIP" --strip-unneeded "$ONYX_DESTINATION/prebuilt/peridot_daemon"
"$ONYX_STRIP" --strip-unneeded "$ONYX_DESTINATION/prebuilt/peridot_ctl"

"$ONYX_APKSIGNER" verify --print-certs-pem "$ONYX_APK" 2>/dev/null \
    | sed -n '/-----BEGIN CERTIFICATE-----/,/-----END CERTIFICATE-----/p' \
    >"$ONYX_DESTINATION/prebuilt/PeridotEssential.x509.pem"
openssl x509 -in "$ONYX_DESTINATION/prebuilt/PeridotEssential.x509.pem" \
    -noout -subject >/dev/null
ONYX_APK_CERT_DIGEST="$("$ONYX_APKSIGNER" verify --print-certs "$ONYX_APK" \
    2>/dev/null | sed -n 's/^.*certificate SHA-256 digest: //p' | head -n 1 \
    | tr '[:upper:]' '[:lower:]')"
ONYX_PEM_CERT_DIGEST="$(openssl x509 \
    -in "$ONYX_DESTINATION/prebuilt/PeridotEssential.x509.pem" -outform DER \
    | sha256sum | awk '{print $1}')"
[[ "$ONYX_APK_CERT_DIGEST" =~ ^[0-9a-f]{64}$ ]] || {
    echo "Unable to read the exported APK certificate" >&2
    exit 1
}
[ "$ONYX_APK_CERT_DIGEST" = "$ONYX_PEM_CERT_DIGEST" ] || {
    echo "Exported APK and SELinux public certificate do not match" >&2
    exit 1
}
openssl x509 -in "$ONYX_DESTINATION/prebuilt/PeridotEssential.x509.pem" \
    -noout -subject -issuer -dates -fingerprint -sha256 \
    >"$ONYX_DESTINATION/SIGNING-CERTIFICATE.txt"

ANDROID_SDK_ROOT="$ONYX_SDK_ROOT" bash "$ONYX_ROOT/tools/check-platform-dex.sh" "$ONYX_DESTINATION/prebuilt/PeridotEssential.apk"

file "$ONYX_DESTINATION/prebuilt/peridot_daemon" | grep -q 'ARM aarch64'
file "$ONYX_DESTINATION/prebuilt/peridot_ctl" | grep -q 'ARM aarch64'

ONYX_DAEMON_NEEDED="$(readelf -d "$ONYX_DESTINATION/prebuilt/peridot_daemon" \
    | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | LC_ALL=C sort)"
ONYX_CTL_NEEDED="$(readelf -d "$ONYX_DESTINATION/prebuilt/peridot_ctl" \
    | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | LC_ALL=C sort)"
ONYX_EXPECTED_DAEMON_NEEDED=$'libbinder_ndk.so\nlibc.so\nlibdl.so'
ONYX_EXPECTED_CTL_NEEDED=$'libc.so\nlibdl.so'
[ "$ONYX_DAEMON_NEEDED" = "$ONYX_EXPECTED_DAEMON_NEEDED" ] || {
    echo "peridot_daemon dependencies do not match Android.bp" >&2
    printf '%s\n' "$ONYX_DAEMON_NEEDED" >&2
    exit 1
}
[ "$ONYX_CTL_NEEDED" = "$ONYX_EXPECTED_CTL_NEEDED" ] || {
    echo "peridot_ctl dependencies do not match Android.bp" >&2
    printf '%s\n' "$ONYX_CTL_NEEDED" >&2
    exit 1
}

(
    cd "$ONYX_DESTINATION"
    find . -type f ! -name SHA256SUMS -print0 \
        | LC_ALL=C sort -z \
        | xargs -0 sha256sum \
        > SHA256SUMS
)

echo "AOSP_EXPORT=PASS"
echo "APK_CERT_SHA256=$ONYX_APK_CERT_DIGEST"
echo "DIRECTORY=$ONYX_DESTINATION"
