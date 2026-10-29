#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Explicitly renamed paths only. A replacement must exist before removing a
# stale source; licenses, notices, credits and author history are untouched.
while IFS='|' read -r old replacement; do
    [ -n "$old" ] || continue
    if [ -f "$ROOT/$old" ]; then
        if [ ! -f "$ROOT/$replacement" ]; then
            echo "Refusing cleanup: replacement missing: $replacement" >&2
            exit 1
        fi
        rm -- "$ROOT/$old"
        echo "Removed stale renamed source: $old"
    fi
done <<'PATHS'
runtime/daemon-rust/src/bin/rodin_essentiald.rs|runtime/daemon-rust/src/bin/peridot_daemon.rs
runtime/daemon-rust/src/bin/rodin_essentialctl.rs|runtime/daemon-rust/src/bin/peridot_ctl.rs
android/platform/io/github/neeschal/rodinessential/RodinActivity.java|android/platform/io/github/davvv98/peridotessential/PeridotActivity.java
android/platform/io/github/neeschal/rodinessential/BypassTileService.java|android/platform/io/github/davvv98/peridotessential/BypassTileService.java
ui/flutter/lib/backend/rodin_backend.dart|ui/flutter/lib/backend/peridot_backend.dart
android/aosp/BoardConfigRodinEssential.mk|android/aosp/BoardConfigPeridotEssential.mk
android/aosp/rodin-essential.mk|android/aosp/peridot-essential.mk
android/aosp/rodin_daemon.rc|android/aosp/peridot_daemon.rc
android/aosp/sepolicy/product/private/rodin_app.te|android/aosp/sepolicy/product/private/peridot_app.te
android/aosp/sepolicy/product/private/rodin_daemon.te|android/aosp/sepolicy/product/private/peridot_daemon.te
android/aosp/sepolicy/product/public/rodin_daemon.te|android/aosp/sepolicy/product/public/peridot_daemon.te
android/aosp/sepolicy/vendor/rodin_daemon.te|android/aosp/sepolicy/vendor/peridot_daemon.te
android/unpacked/rodin-native.cil|android/unpacked/peridot-native.cil
PATHS
