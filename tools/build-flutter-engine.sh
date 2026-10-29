#!/usr/bin/env bash
set -euo pipefail

ONYX_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ONYX_FLUTTER_BIN="$(command -v flutter 2>/dev/null || true)"

[ -n "$ONYX_FLUTTER_BIN" ] || {
    echo "Flutter is not in PATH" >&2
    exit 1
}

ONYX_FLUTTER_BIN="$(readlink -f "$ONYX_FLUTTER_BIN")"
ONYX_FLUTTER_ROOT="$(cd "$(dirname "$ONYX_FLUTTER_BIN")/.." && pwd)"
ONYX_ENGINE_SRC="${ONYX_FLUTTER_ENGINE_SRC:-$ONYX_FLUTTER_ROOT/engine/src}"
ONYX_ENGINE_OUT="$ONYX_ENGINE_SRC/out/android_release_arm64"
ONYX_PREBUILT="$ONYX_ROOT/runtime/flutter-engine/prebuilt/android-arm64"

[ -x "$ONYX_ENGINE_SRC/flutter/tools/gn" ] || {
    echo "Flutter engine checkout not found: $ONYX_ENGINE_SRC" >&2
    echo "Set ONYX_FLUTTER_ENGINE_SRC to the engine src directory" >&2
    exit 1
}

command -v ninja >/dev/null 2>&1 || {
    echo "ninja is required" >&2
    exit 1
}

ONYX_EXPECTED_REVISION="$(tr -d '[:space:]' <"$ONYX_FLUTTER_ROOT/bin/internal/engine.version")"
ONYX_ACTUAL_REVISION="$(git -C "$ONYX_ENGINE_SRC" rev-parse HEAD 2>/dev/null || true)"

if [ -n "$ONYX_ACTUAL_REVISION" ] && [ "$ONYX_ACTUAL_REVISION" != "$ONYX_EXPECTED_REVISION" ]; then
    echo "Flutter framework and engine revisions do not match" >&2
    echo "framework engine: $ONYX_EXPECTED_REVISION" >&2
    echo "engine checkout:  $ONYX_ACTUAL_REVISION" >&2
    exit 1
fi

(
    cd "$ONYX_ENGINE_SRC"
    ./flutter/tools/gn \
        --android \
        --android-cpu arm64 \
        --runtime-mode release \
        --no-lto
)

ninja -C "$ONYX_ENGINE_OUT" \
    flutter/shell/platform/embedder:flutter_engine_library

for ONYX_ARTIFACT in \
    "$ONYX_ENGINE_OUT/libflutter_engine.so" \
    "$ONYX_ENGINE_OUT/icudtl.dat" \
    "$ONYX_ENGINE_SRC/flutter/shell/platform/embedder/embedder.h"; do
    [ -f "$ONYX_ARTIFACT" ] || {
        echo "Missing engine artifact: $ONYX_ARTIFACT" >&2
        exit 1
    }
done

mkdir -p "$ONYX_PREBUILT" "$ONYX_ROOT/runtime/flutter-engine/include"
install -m 0644 "$ONYX_ENGINE_OUT/libflutter_engine.so" "$ONYX_PREBUILT/libflutter_engine.so"
install -m 0644 "$ONYX_ENGINE_OUT/icudtl.dat" "$ONYX_PREBUILT/icudtl.dat"
install -m 0644 \
    "$ONYX_ENGINE_SRC/flutter/shell/platform/embedder/embedder.h" \
    "$ONYX_ROOT/runtime/flutter-engine/include/embedder.h"

echo "FLUTTER_ENGINE_BUILD=PASS"
echo "ENGINE_REVISION=$ONYX_EXPECTED_REVISION"
echo "PREBUILT_DIR=$ONYX_PREBUILT"
