#!/usr/bin/env bash
set -euo pipefail

ONYX_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ONYX_MODULE="$ONYX_ROOT/android/kernelsu-next/module.prop"
ONYX_MANIFEST="$ONYX_ROOT/android/package/AndroidManifest.xml"
ONYX_FLUTTER="$ONYX_ROOT/ui/flutter/pubspec.yaml"
ONYX_MAIN="$ONYX_ROOT/ui/flutter/lib/main.dart"
ONYX_DAEMON="$ONYX_ROOT/runtime/daemon-rust/src/lib.rs"
ONYX_HOST="$ONYX_ROOT/runtime/host-rust/src/backend_bridge.rs"

ONYX_CARGO_VERSION="$(sed -n 's/^version = "\([0-9][0-9.]*\)"$/\1/p' \
    "$ONYX_ROOT/Cargo.toml" | head -n 1)"
ONYX_MODULE_VERSION="$(sed -n 's/^version=v//p' "$ONYX_MODULE")"
ONYX_MODULE_CODE="$(sed -n 's/^versionCode=//p' "$ONYX_MODULE")"
ONYX_MANIFEST_VERSION="$(sed -n 's/.*android:versionName="\([^"]*\)".*/\1/p' \
    "$ONYX_MANIFEST")"
ONYX_MANIFEST_CODE="$(sed -n 's/.*android:versionCode="\([^"]*\)".*/\1/p' \
    "$ONYX_MANIFEST")"
ONYX_FLUTTER_VERSION="$(sed -n 's/^version: \([^+]*\)+.*/\1/p' "$ONYX_FLUTTER")"
ONYX_FLUTTER_CODE="$(sed -n 's/^version: [^+]*+\([0-9]*\).*/\1/p' "$ONYX_FLUTTER")"
ONYX_CHANGELOG_VERSION="$(sed -n 's/^## \([0-9][0-9.]*\)$/\1/p' \
    "$ONYX_ROOT/CHANGELOG.md" | head -n 1)"

for ONYX_VERSION in \
    "$ONYX_MODULE_VERSION" "$ONYX_MANIFEST_VERSION" \
    "$ONYX_FLUTTER_VERSION" "$ONYX_CHANGELOG_VERSION"; do
    [ "$ONYX_VERSION" = "$ONYX_CARGO_VERSION" ] || {
        echo "Release version mismatch: $ONYX_VERSION / $ONYX_CARGO_VERSION" >&2
        exit 1
    }
done

for ONYX_CODE in "$ONYX_MODULE_CODE" "$ONYX_MANIFEST_CODE" "$ONYX_FLUTTER_CODE"; do
    [ "$ONYX_CODE" = "$ONYX_MODULE_CODE" ] || {
        echo "Release versionCode mismatch: $ONYX_CODE / $ONYX_MODULE_CODE" >&2
        exit 1
    }
done

IFS=. read -r ONYX_MAJOR ONYX_MINOR ONYX_PATCH <<EOF
$ONYX_CARGO_VERSION
EOF
ONYX_EXPECTED_CODE="$(printf '%d%02d%02d' \
    "$ONYX_MAJOR" "$ONYX_MINOR" "$ONYX_PATCH")"
[ "$ONYX_MODULE_CODE" = "$ONYX_EXPECTED_CODE" ] || {
    echo "Release versionCode is not derived from the semantic version" >&2
    exit 1
}

grep -Fq "const String peridotVersionName = '$ONYX_CARGO_VERSION';" \
    "$ONYX_ROOT/ui/flutter/lib/release_identity.dart" && \
grep -Fq "const int peridotVersionCode = $ONYX_MODULE_CODE;" \
    "$ONYX_ROOT/ui/flutter/lib/release_identity.dart" && \
grep -Fq "version: 'v\$peridotVersionName'" "$ONYX_MAIN" || {
    echo "In-app release label is not synchronized" >&2
    exit 1
}

ONYX_DAEMON_SOCKET="$(sed -n 's/^pub const SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$ONYX_DAEMON")"
ONYX_HOST_SOCKET="$(sed -n 's/^const SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$ONYX_HOST")"
ONYX_DAEMON_REVERSE="$(sed -n 's/^pub const REVERSE_SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$ONYX_DAEMON")"
ONYX_HOST_REVERSE="$(sed -n 's/^const REVERSE_SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$ONYX_HOST")"
ONYX_PROTOCOL="$(sed -n 's/^pub const PROTOCOL_VERSION: &str = "\([^"]*\)";/\1/p' \
    "$ONYX_DAEMON")"

[ -n "$ONYX_DAEMON_SOCKET" ] && [ "$ONYX_DAEMON_SOCKET" = "$ONYX_HOST_SOCKET" ] || {
    echo "App/daemon primary socket mismatch" >&2
    exit 1
}
[ -n "$ONYX_DAEMON_REVERSE" ] && [ "$ONYX_DAEMON_REVERSE" = "$ONYX_HOST_REVERSE" ] || {
    echo "App/daemon reverse socket mismatch" >&2
    exit 1
}
grep -Fq "Some(\"$ONYX_PROTOCOL\")" "$ONYX_HOST" || {
    echo "App/daemon protocol mismatch" >&2
    exit 1
}

for ONYX_PACKAGER in \
    "$ONYX_ROOT/tools/build-kernelsu-next-module.sh" \
    "$ONYX_ROOT/tools/export-aosp-bundle.sh"; do
    grep -Fq 'build-and-install.sh' "$ONYX_PACKAGER"
    grep -Fq 'Peridot-Essential.apk' "$ONYX_PACKAGER"
    grep -Fq 'host-cargo/aarch64-linux-android/release/peridot_daemon' "$ONYX_PACKAGER"
    grep -Fq -- '--strip-unneeded' "$ONYX_PACKAGER"
done

echo 'RELEASE_SYNC_CONTRACT_TEST=PASS'
