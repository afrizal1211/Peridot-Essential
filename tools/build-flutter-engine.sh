#!/usr/bin/env bash
set -euo pipefail

PERIDOT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PERIDOT_FLUTTER_BIN="$(command -v flutter 2>/dev/null || true)"

[ -n "$PERIDOT_FLUTTER_BIN" ] || {
    echo "Flutter is not in PATH" >&2
    exit 1
}

PERIDOT_FLUTTER_BIN="$(readlink -f "$PERIDOT_FLUTTER_BIN")"
PERIDOT_FLUTTER_ROOT="$(cd "$(dirname "$PERIDOT_FLUTTER_BIN")/.." && pwd)"
PERIDOT_ENGINE_SRC="${PERIDOT_FLUTTER_ENGINE_SRC:-$PERIDOT_FLUTTER_ROOT/engine/src}"
PERIDOT_ENGINE_OUT="$PERIDOT_ENGINE_SRC/out/android_release_arm64"
PERIDOT_PREBUILT="$PERIDOT_ROOT/runtime/flutter-engine/prebuilt/android-arm64"

[ -x "$PERIDOT_ENGINE_SRC/flutter/tools/gn" ] || {
    echo "Flutter engine checkout not found: $PERIDOT_ENGINE_SRC" >&2
    echo "Set PERIDOT_FLUTTER_ENGINE_SRC to the engine src directory" >&2
    exit 1
}

command -v ninja >/dev/null 2>&1 || {
    echo "ninja is required" >&2
    exit 1
}

PERIDOT_EXPECTED_REVISION="$(tr -d '[:space:]' <"$PERIDOT_FLUTTER_ROOT/bin/internal/engine.version")"
PERIDOT_ACTUAL_REVISION="$(git -C "$PERIDOT_ENGINE_SRC" rev-parse HEAD 2>/dev/null || true)"

if [ -n "$PERIDOT_ACTUAL_REVISION" ] && [ "$PERIDOT_ACTUAL_REVISION" != "$PERIDOT_EXPECTED_REVISION" ]; then
    echo "Flutter framework and engine revisions do not match" >&2
    echo "framework engine: $PERIDOT_EXPECTED_REVISION" >&2
    echo "engine checkout:  $PERIDOT_ACTUAL_REVISION" >&2
    exit 1
fi

(
    cd "$PERIDOT_ENGINE_SRC"
    ./flutter/tools/gn \
        --android \
        --android-cpu arm64 \
        --runtime-mode release \
        --no-lto
)

ninja -C "$PERIDOT_ENGINE_OUT" \
    flutter/shell/platform/embedder:flutter_engine_library

for PERIDOT_ARTIFACT in \
    "$PERIDOT_ENGINE_OUT/libflutter_engine.so" \
    "$PERIDOT_ENGINE_OUT/icudtl.dat" \
    "$PERIDOT_ENGINE_SRC/flutter/shell/platform/embedder/embedder.h"; do
    [ -f "$PERIDOT_ARTIFACT" ] || {
        echo "Missing engine artifact: $PERIDOT_ARTIFACT" >&2
        exit 1
    }
done

mkdir -p "$PERIDOT_PREBUILT" "$PERIDOT_ROOT/runtime/flutter-engine/include"
install -m 0644 "$PERIDOT_ENGINE_OUT/libflutter_engine.so" "$PERIDOT_PREBUILT/libflutter_engine.so"
install -m 0644 "$PERIDOT_ENGINE_OUT/icudtl.dat" "$PERIDOT_PREBUILT/icudtl.dat"
install -m 0644 \
    "$PERIDOT_ENGINE_SRC/flutter/shell/platform/embedder/embedder.h" \
    "$PERIDOT_ROOT/runtime/flutter-engine/include/embedder.h"

echo "FLUTTER_ENGINE_BUILD=PASS"
echo "ENGINE_REVISION=$PERIDOT_EXPECTED_REVISION"
echo "PREBUILT_DIR=$PERIDOT_PREBUILT"
