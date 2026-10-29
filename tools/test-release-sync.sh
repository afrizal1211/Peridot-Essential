#!/usr/bin/env bash
set -euo pipefail

PERIDOT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PERIDOT_MODULE="$PERIDOT_ROOT/android/kernelsu-next/module.prop"
PERIDOT_MANIFEST="$PERIDOT_ROOT/android/package/AndroidManifest.xml"
PERIDOT_FLUTTER="$PERIDOT_ROOT/ui/flutter/pubspec.yaml"
PERIDOT_MAIN="$PERIDOT_ROOT/ui/flutter/lib/main.dart"
PERIDOT_DAEMON="$PERIDOT_ROOT/runtime/daemon-rust/src/lib.rs"
PERIDOT_HOST="$PERIDOT_ROOT/runtime/host-rust/src/backend_bridge.rs"

PERIDOT_CARGO_VERSION="$(sed -n 's/^version = "\([0-9][0-9.]*\)"$/\1/p' \
    "$PERIDOT_ROOT/Cargo.toml" | head -n 1)"
PERIDOT_MODULE_VERSION="$(sed -n 's/^version=v//p' "$PERIDOT_MODULE")"
PERIDOT_MODULE_CODE="$(sed -n 's/^versionCode=//p' "$PERIDOT_MODULE")"
PERIDOT_MANIFEST_VERSION="$(sed -n 's/.*android:versionName="\([^"]*\)".*/\1/p' \
    "$PERIDOT_MANIFEST")"
PERIDOT_MANIFEST_CODE="$(sed -n 's/.*android:versionCode="\([^"]*\)".*/\1/p' \
    "$PERIDOT_MANIFEST")"
PERIDOT_FLUTTER_VERSION="$(sed -n 's/^version: \([^+]*\)+.*/\1/p' "$PERIDOT_FLUTTER")"
PERIDOT_FLUTTER_CODE="$(sed -n 's/^version: [^+]*+\([0-9]*\).*/\1/p' "$PERIDOT_FLUTTER")"
PERIDOT_CHANGELOG_VERSION="$(sed -n 's/^## \([0-9][0-9.]*\)$/\1/p' \
    "$PERIDOT_ROOT/CHANGELOG.md" | head -n 1)"

for PERIDOT_VERSION in \
    "$PERIDOT_MODULE_VERSION" "$PERIDOT_MANIFEST_VERSION" \
    "$PERIDOT_FLUTTER_VERSION" "$PERIDOT_CHANGELOG_VERSION"; do
    [ "$PERIDOT_VERSION" = "$PERIDOT_CARGO_VERSION" ] || {
        echo "Release version mismatch: $PERIDOT_VERSION / $PERIDOT_CARGO_VERSION" >&2
        exit 1
    }
done

for PERIDOT_CODE in "$PERIDOT_MODULE_CODE" "$PERIDOT_MANIFEST_CODE" "$PERIDOT_FLUTTER_CODE"; do
    [ "$PERIDOT_CODE" = "$PERIDOT_MODULE_CODE" ] || {
        echo "Release versionCode mismatch: $PERIDOT_CODE / $PERIDOT_MODULE_CODE" >&2
        exit 1
    }
done

IFS=. read -r PERIDOT_MAJOR PERIDOT_MINOR PERIDOT_PATCH <<EOF
$PERIDOT_CARGO_VERSION
EOF
PERIDOT_EXPECTED_CODE="$(printf '%d%02d%02d' \
    "$PERIDOT_MAJOR" "$PERIDOT_MINOR" "$PERIDOT_PATCH")"
[ "$PERIDOT_MODULE_CODE" = "$PERIDOT_EXPECTED_CODE" ] || {
    echo "Release versionCode is not derived from the semantic version" >&2
    exit 1
}

grep -Fq "const String peridotVersionName = '$PERIDOT_CARGO_VERSION';" \
    "$PERIDOT_ROOT/ui/flutter/lib/release_identity.dart" && \
grep -Fq "const int peridotVersionCode = $PERIDOT_MODULE_CODE;" \
    "$PERIDOT_ROOT/ui/flutter/lib/release_identity.dart" && \
grep -Fq "version: 'v\$peridotVersionName'" "$PERIDOT_MAIN" || {
    echo "In-app release label is not synchronized" >&2
    exit 1
}

PERIDOT_DAEMON_SOCKET="$(sed -n 's/^pub const SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$PERIDOT_DAEMON")"
PERIDOT_HOST_SOCKET="$(sed -n 's/^const SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$PERIDOT_HOST")"
PERIDOT_DAEMON_REVERSE="$(sed -n 's/^pub const REVERSE_SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$PERIDOT_DAEMON")"
PERIDOT_HOST_REVERSE="$(sed -n 's/^const REVERSE_SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$PERIDOT_HOST")"
PERIDOT_PROTOCOL="$(sed -n 's/^pub const PROTOCOL_VERSION: &str = "\([^"]*\)";/\1/p' \
    "$PERIDOT_DAEMON")"

[ -n "$PERIDOT_DAEMON_SOCKET" ] && [ "$PERIDOT_DAEMON_SOCKET" = "$PERIDOT_HOST_SOCKET" ] || {
    echo "App/daemon primary socket mismatch" >&2
    exit 1
}
[ -n "$PERIDOT_DAEMON_REVERSE" ] && [ "$PERIDOT_DAEMON_REVERSE" = "$PERIDOT_HOST_REVERSE" ] || {
    echo "App/daemon reverse socket mismatch" >&2
    exit 1
}
grep -Fq "Some(\"$PERIDOT_PROTOCOL\")" "$PERIDOT_HOST" || {
    echo "App/daemon protocol mismatch" >&2
    exit 1
}

for PERIDOT_PACKAGER in \
    "$PERIDOT_ROOT/tools/build-kernelsu-next-module.sh" \
    "$PERIDOT_ROOT/tools/export-aosp-bundle.sh"; do
    grep -Fq 'build-and-install.sh' "$PERIDOT_PACKAGER"
    grep -Fq 'Peridot-Essential.apk' "$PERIDOT_PACKAGER"
    grep -Fq 'host-cargo/aarch64-linux-android/release/peridot_daemon' "$PERIDOT_PACKAGER"
    grep -Fq -- '--strip-unneeded' "$PERIDOT_PACKAGER"
done

echo 'RELEASE_SYNC_CONTRACT_TEST=PASS'
