#!/usr/bin/env bash
set -euo pipefail

PERIDOT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PERIDOT_STAMP="$(date +%Y%m%d-%H%M%S)"
PERIDOT_DESTINATION="${1:-$PERIDOT_ROOT/dist/aosp/PeridotEssential-$PERIDOT_STAMP}"
PERIDOT_BUILD_DIR="$PERIDOT_ROOT/out/aosp-export/$PERIDOT_STAMP/build"
PERIDOT_TEMPLATE="$PERIDOT_ROOT/android/aosp"
PERIDOT_SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
PERIDOT_NDK_VERSION="$(find "$PERIDOT_SDK_ROOT/ndk" -mindepth 1 -maxdepth 1 \
    -type d -printf '%f\n' | sort -V | tail -1)"
PERIDOT_STRIP="$PERIDOT_SDK_ROOT/ndk/$PERIDOT_NDK_VERSION/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"

[ -n "${PERIDOT_KEYSTORE:-}" ] || {
    echo "PERIDOT_KEYSTORE is required for a reproducibly signed AOSP bundle." >&2
    echo "ROM maintainers must keep the same private key for every update." >&2
    exit 1
}
[ -f "$PERIDOT_KEYSTORE" ] || {
    echo "PERIDOT_KEYSTORE does not exist: $PERIDOT_KEYSTORE" >&2
    exit 1
}

for PERIDOT_TOOL in openssl readelf sha256sum unzip; do
    command -v "$PERIDOT_TOOL" >/dev/null 2>&1 || {
        echo "Missing AOSP export tool: $PERIDOT_TOOL" >&2
        exit 1
    }
done
[ -x "$PERIDOT_STRIP" ] || {
    echo "Missing Android LLVM strip tool: $PERIDOT_STRIP" >&2
    exit 1
}

"$PERIDOT_ROOT/tools/test-aosp-integration.sh"
"$PERIDOT_ROOT/tools/test-release-sync.sh"

if [ -e "$PERIDOT_DESTINATION" ]; then
    echo "Destination already exists: $PERIDOT_DESTINATION" >&2
    exit 1
fi

PERIDOT_BUILD_ONLY=1 \
PERIDOT_OUTPUT_DIR="$PERIDOT_BUILD_DIR" \
    "$PERIDOT_ROOT/build-and-install.sh"

PERIDOT_APK="$PERIDOT_BUILD_DIR/Peridot-Essential.apk"
PERIDOT_DAEMON="$PERIDOT_BUILD_DIR/host-cargo/aarch64-linux-android/release/peridot_daemon"
PERIDOT_CTL="$PERIDOT_BUILD_DIR/host-cargo/aarch64-linux-android/release/peridot_ctl"
PERIDOT_BUILD_TOOLS="$(find "$PERIDOT_SDK_ROOT/build-tools" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V | tail -1)"
PERIDOT_APKSIGNER="$PERIDOT_SDK_ROOT/build-tools/$PERIDOT_BUILD_TOOLS/apksigner"

[ -x "$PERIDOT_APKSIGNER" ] || {
    echo "Missing Android APK signer: $PERIDOT_APKSIGNER" >&2
    exit 1
}

for PERIDOT_ARTIFACT in "$PERIDOT_APK" "$PERIDOT_DAEMON" "$PERIDOT_CTL"; do
    [ -f "$PERIDOT_ARTIFACT" ] || {
        echo "Missing AOSP artifact: $PERIDOT_ARTIFACT" >&2
        exit 1
    }
done

mkdir -p "$PERIDOT_DESTINATION/prebuilt" "$PERIDOT_DESTINATION/docs"
cp -a "$PERIDOT_TEMPLATE/Android.bp" "$PERIDOT_DESTINATION/Android.bp"
cp -a "$PERIDOT_TEMPLATE/peridot-essential.mk" "$PERIDOT_DESTINATION/peridot-essential.mk"
cp -a "$PERIDOT_TEMPLATE/BoardConfigPeridotEssential.mk" "$PERIDOT_DESTINATION/BoardConfigPeridotEssential.mk"
cp -a "$PERIDOT_TEMPLATE/peridot_daemon.rc" "$PERIDOT_DESTINATION/peridot_daemon.rc"
cp -a "$PERIDOT_TEMPLATE/sepolicy" "$PERIDOT_DESTINATION/sepolicy"
cp -a "$PERIDOT_TEMPLATE/README.md" "$PERIDOT_DESTINATION/README.md"
cp -a "$PERIDOT_ROOT/docs/ROM_INTEGRATION.md" "$PERIDOT_DESTINATION/docs/ROM_INTEGRATION.md"
cp -a "$PERIDOT_ROOT/docs/UNPACKED_ROM_INTEGRATION.md" "$PERIDOT_DESTINATION/docs/UNPACKED_ROM_INTEGRATION.md"
cp -a "$PERIDOT_ROOT/docs/UNPACKED_ROM_PORTER_GUIDE.md" "$PERIDOT_DESTINATION/docs/UNPACKED_ROM_PORTER_GUIDE.md"
mkdir -p "$PERIDOT_DESTINATION/tools"
cp -a "$PERIDOT_ROOT/tools/prepare-aosp-touch-policy.py" "$PERIDOT_DESTINATION/tools/prepare-aosp-touch-policy.py"
install -m 0644 "$PERIDOT_APK" "$PERIDOT_DESTINATION/prebuilt/PeridotEssential.apk"
install -m 0755 "$PERIDOT_DAEMON" "$PERIDOT_DESTINATION/prebuilt/peridot_daemon"
install -m 0755 "$PERIDOT_CTL" "$PERIDOT_DESTINATION/prebuilt/peridot_ctl"
"$PERIDOT_STRIP" --strip-unneeded "$PERIDOT_DESTINATION/prebuilt/peridot_daemon"
"$PERIDOT_STRIP" --strip-unneeded "$PERIDOT_DESTINATION/prebuilt/peridot_ctl"

"$PERIDOT_APKSIGNER" verify --print-certs-pem "$PERIDOT_APK" 2>/dev/null \
    | sed -n '/-----BEGIN CERTIFICATE-----/,/-----END CERTIFICATE-----/p' \
    >"$PERIDOT_DESTINATION/prebuilt/PeridotEssential.x509.pem"
openssl x509 -in "$PERIDOT_DESTINATION/prebuilt/PeridotEssential.x509.pem" \
    -noout -subject >/dev/null
PERIDOT_APK_CERT_DIGEST="$("$PERIDOT_APKSIGNER" verify --print-certs "$PERIDOT_APK" \
    2>/dev/null | sed -n 's/^.*certificate SHA-256 digest: //p' | head -n 1 \
    | tr '[:upper:]' '[:lower:]')"
PERIDOT_PEM_CERT_DIGEST="$(openssl x509 \
    -in "$PERIDOT_DESTINATION/prebuilt/PeridotEssential.x509.pem" -outform DER \
    | sha256sum | awk '{print $1}')"
[[ "$PERIDOT_APK_CERT_DIGEST" =~ ^[0-9a-f]{64}$ ]] || {
    echo "Unable to read the exported APK certificate" >&2
    exit 1
}
[ "$PERIDOT_APK_CERT_DIGEST" = "$PERIDOT_PEM_CERT_DIGEST" ] || {
    echo "Exported APK and SELinux public certificate do not match" >&2
    exit 1
}
openssl x509 -in "$PERIDOT_DESTINATION/prebuilt/PeridotEssential.x509.pem" \
    -noout -subject -issuer -dates -fingerprint -sha256 \
    >"$PERIDOT_DESTINATION/SIGNING-CERTIFICATE.txt"

ANDROID_SDK_ROOT="$PERIDOT_SDK_ROOT" bash "$PERIDOT_ROOT/tools/check-platform-dex.sh" "$PERIDOT_DESTINATION/prebuilt/PeridotEssential.apk"

file "$PERIDOT_DESTINATION/prebuilt/peridot_daemon" | grep -q 'ARM aarch64'
file "$PERIDOT_DESTINATION/prebuilt/peridot_ctl" | grep -q 'ARM aarch64'

PERIDOT_DAEMON_NEEDED="$(readelf -d "$PERIDOT_DESTINATION/prebuilt/peridot_daemon" \
    | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | LC_ALL=C sort)"
PERIDOT_CTL_NEEDED="$(readelf -d "$PERIDOT_DESTINATION/prebuilt/peridot_ctl" \
    | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | LC_ALL=C sort)"
PERIDOT_EXPECTED_DAEMON_NEEDED=$'libbinder_ndk.so\nlibc.so\nlibdl.so'
PERIDOT_EXPECTED_CTL_NEEDED=$'libc.so\nlibdl.so'
[ "$PERIDOT_DAEMON_NEEDED" = "$PERIDOT_EXPECTED_DAEMON_NEEDED" ] || {
    echo "peridot_daemon dependencies do not match Android.bp" >&2
    printf '%s\n' "$PERIDOT_DAEMON_NEEDED" >&2
    exit 1
}
[ "$PERIDOT_CTL_NEEDED" = "$PERIDOT_EXPECTED_CTL_NEEDED" ] || {
    echo "peridot_ctl dependencies do not match Android.bp" >&2
    printf '%s\n' "$PERIDOT_CTL_NEEDED" >&2
    exit 1
}

(
    cd "$PERIDOT_DESTINATION"
    find . -type f ! -name SHA256SUMS -print0 \
        | LC_ALL=C sort -z \
        | xargs -0 sha256sum \
        > SHA256SUMS
)

echo "AOSP_EXPORT=PASS"
echo "APK_CERT_SHA256=$PERIDOT_APK_CERT_DIGEST"
echo "DIRECTORY=$PERIDOT_DESTINATION"
