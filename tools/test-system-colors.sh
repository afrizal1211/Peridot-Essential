#!/usr/bin/env bash
set -euo pipefail

PERIDOT_PALETTE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PERIDOT_PALETTE_TEST_DIR="$(mktemp -d -t peridot-system-colors.XXXXXXXX)"
trap 'rm -f "$PERIDOT_PALETTE_TEST_DIR/host-tests"; rmdir "$PERIDOT_PALETTE_TEST_DIR"' EXIT

cargo test --manifest-path "$PERIDOT_PALETTE_ROOT/Cargo.toml" \
    -p peridot-essential-daemon system_colors::tests
rustc --edition=2024 --test "$PERIDOT_PALETTE_ROOT/tools/tests/system-colors-host.rs" \
    -o "$PERIDOT_PALETTE_TEST_DIR/host-tests"
"$PERIDOT_PALETTE_TEST_DIR/host-tests"

cd "$PERIDOT_PALETTE_ROOT/ui/flutter"
flutter test test/system_colors_test.dart test/system_colors_preview_test.dart \
    test/system_colors_monitor_test.dart test/backend_connection_test.dart
