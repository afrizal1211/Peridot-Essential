#!/usr/bin/env bash
set -euo pipefail

PERIDOT_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

usage() {
    cat <<'EOF'
Usage:
  tools/integrate-aosp-rom.sh <aosp-root>
  tools/integrate-aosp-rom.sh <aosp-root> <product-makefile> <boardconfig>

Builds and verifies Peridot Essential, then stages a self-contained integration
at vendor/peridot-essential in an existing AOSP ROM source tree.

With the optional device-tree file arguments, the script also adds both exact
include lines idempotently. Paths may be absolute or relative to <aosp-root>.
With only <aosp-root>, it stages the bundle and prints the two lines.
EOF
}

if [ "$#" -ne 1 ] && [ "$#" -ne 3 ]; then
    usage >&2
    exit 2
fi

PERIDOT_AOSP_ROOT="$(readlink -f "$1")"
PERIDOT_DESTINATION="$PERIDOT_AOSP_ROOT/vendor/peridot-essential"

[ -f "$PERIDOT_AOSP_ROOT/build/make/core/envsetup.mk" ] || {
    echo "Not an AOSP source root: $PERIDOT_AOSP_ROOT" >&2
    exit 1
}

if [ -e "$PERIDOT_DESTINATION" ]; then
    echo "Destination already exists: $PERIDOT_DESTINATION" >&2
    echo "Remove or archive it explicitly before staging a replacement." >&2
    exit 1
fi

PERIDOT_PRODUCT_INCLUDE='$(call inherit-product, vendor/peridot-essential/peridot-essential.mk)'
PERIDOT_BOARD_INCLUDE='include vendor/peridot-essential/BoardConfigPeridotEssential.mk'
PERIDOT_WIRED=0

resolve_tree_file() {
    local PERIDOT_REQUESTED="$1"
    local PERIDOT_RESOLVED
    case "$PERIDOT_REQUESTED" in
        /*) PERIDOT_RESOLVED="$(readlink -f "$PERIDOT_REQUESTED")" ;;
        *) PERIDOT_RESOLVED="$(readlink -f "$PERIDOT_AOSP_ROOT/$PERIDOT_REQUESTED")" ;;
    esac
    case "$PERIDOT_RESOLVED" in
        "$PERIDOT_AOSP_ROOT"/*) ;;
        *)
            echo "Device-tree file is outside the AOSP root: $PERIDOT_REQUESTED" >&2
            exit 1
            ;;
    esac
    [ -f "$PERIDOT_RESOLVED" ] || {
        echo "Device-tree file does not exist: $PERIDOT_RESOLVED" >&2
        exit 1
    }
    printf '%s\n' "$PERIDOT_RESOLVED"
}

append_include_once() {
    local PERIDOT_FILE="$1"
    local PERIDOT_LINE="$2"
    if grep -Fxq "$PERIDOT_LINE" "$PERIDOT_FILE"; then
        return 0
    fi
    printf '\n# Peridot Essential\n%s\n' "$PERIDOT_LINE" >>"$PERIDOT_FILE"
}

if [ "$#" -eq 3 ]; then
    PERIDOT_PRODUCT_FILE="$(resolve_tree_file "$2")"
    PERIDOT_BOARD_FILE="$(resolve_tree_file "$3")"
    for PERIDOT_DEVICE_TREE_FILE in "$PERIDOT_PRODUCT_FILE" "$PERIDOT_BOARD_FILE"; do
        [ -w "$PERIDOT_DEVICE_TREE_FILE" ] || {
            echo "Device-tree file is not writable: $PERIDOT_DEVICE_TREE_FILE" >&2
            exit 1
        }
    done
fi

"$PERIDOT_PROJECT_ROOT/tools/export-aosp-bundle.sh" "$PERIDOT_DESTINATION"

if [ "$#" -eq 3 ]; then
    append_include_once "$PERIDOT_PRODUCT_FILE" "$PERIDOT_PRODUCT_INCLUDE"
    append_include_once "$PERIDOT_BOARD_FILE" "$PERIDOT_BOARD_INCLUDE"
    PERIDOT_WIRED=1
fi

cat <<EOF

AOSP_INTEGRATION=PASS
STAGED_AT=$PERIDOT_DESTINATION
DEVICE_TREE_WIRED=$PERIDOT_WIRED
EOF

if [ "$PERIDOT_WIRED" -eq 0 ]; then
    cat <<EOF
Add this line to the Peridot product makefile:

  $PERIDOT_PRODUCT_INCLUDE

Add this line to device/xiaomi/peridot/BoardConfig.mk:

  $PERIDOT_BOARD_INCLUDE
EOF
fi

cat <<EOF
Before building, review the native touch platform exception:

  python3 vendor/peridot-essential/tools/prepare-aosp-touch-policy.py "$PERIDOT_AOSP_ROOT"

This is a preview, not an automatic policy modification. Follow the staged
docs/ROM_INTEGRATION.md and apply only after maintainer review.
Then run the normal ROM build. For a focused validation first:

  m PeridotEssential peridot_daemon peridot_ctl selinux_policy
EOF
