#!/usr/bin/env bash
set -euo pipefail

ONYX_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

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

ONYX_AOSP_ROOT="$(readlink -f "$1")"
ONYX_DESTINATION="$ONYX_AOSP_ROOT/vendor/peridot-essential"

[ -f "$ONYX_AOSP_ROOT/build/make/core/envsetup.mk" ] || {
    echo "Not an AOSP source root: $ONYX_AOSP_ROOT" >&2
    exit 1
}

if [ -e "$ONYX_DESTINATION" ]; then
    echo "Destination already exists: $ONYX_DESTINATION" >&2
    echo "Remove or archive it explicitly before staging a replacement." >&2
    exit 1
fi

ONYX_PRODUCT_INCLUDE='$(call inherit-product, vendor/peridot-essential/peridot-essential.mk)'
ONYX_BOARD_INCLUDE='include vendor/peridot-essential/BoardConfigPeridotEssential.mk'
ONYX_WIRED=0

resolve_tree_file() {
    local ONYX_REQUESTED="$1"
    local ONYX_RESOLVED
    case "$ONYX_REQUESTED" in
        /*) ONYX_RESOLVED="$(readlink -f "$ONYX_REQUESTED")" ;;
        *) ONYX_RESOLVED="$(readlink -f "$ONYX_AOSP_ROOT/$ONYX_REQUESTED")" ;;
    esac
    case "$ONYX_RESOLVED" in
        "$ONYX_AOSP_ROOT"/*) ;;
        *)
            echo "Device-tree file is outside the AOSP root: $ONYX_REQUESTED" >&2
            exit 1
            ;;
    esac
    [ -f "$ONYX_RESOLVED" ] || {
        echo "Device-tree file does not exist: $ONYX_RESOLVED" >&2
        exit 1
    }
    printf '%s\n' "$ONYX_RESOLVED"
}

append_include_once() {
    local ONYX_FILE="$1"
    local ONYX_LINE="$2"
    if grep -Fxq "$ONYX_LINE" "$ONYX_FILE"; then
        return 0
    fi
    printf '\n# Peridot Essential\n%s\n' "$ONYX_LINE" >>"$ONYX_FILE"
}

if [ "$#" -eq 3 ]; then
    ONYX_PRODUCT_FILE="$(resolve_tree_file "$2")"
    ONYX_BOARD_FILE="$(resolve_tree_file "$3")"
    for ONYX_DEVICE_TREE_FILE in "$ONYX_PRODUCT_FILE" "$ONYX_BOARD_FILE"; do
        [ -w "$ONYX_DEVICE_TREE_FILE" ] || {
            echo "Device-tree file is not writable: $ONYX_DEVICE_TREE_FILE" >&2
            exit 1
        }
    done
fi

"$ONYX_PROJECT_ROOT/tools/export-aosp-bundle.sh" "$ONYX_DESTINATION"

if [ "$#" -eq 3 ]; then
    append_include_once "$ONYX_PRODUCT_FILE" "$ONYX_PRODUCT_INCLUDE"
    append_include_once "$ONYX_BOARD_FILE" "$ONYX_BOARD_INCLUDE"
    ONYX_WIRED=1
fi

cat <<EOF

AOSP_INTEGRATION=PASS
STAGED_AT=$ONYX_DESTINATION
DEVICE_TREE_WIRED=$ONYX_WIRED
EOF

if [ "$ONYX_WIRED" -eq 0 ]; then
    cat <<EOF
Add this line to the Peridot product makefile:

  $ONYX_PRODUCT_INCLUDE

Add this line to device/xiaomi/peridot/BoardConfig.mk:

  $ONYX_BOARD_INCLUDE
EOF
fi

cat <<EOF
Before building, review the native touch platform exception:

  python3 vendor/peridot-essential/tools/prepare-aosp-touch-policy.py "$ONYX_AOSP_ROOT"

This is a preview, not an automatic policy modification. Follow the staged
docs/ROM_INTEGRATION.md and apply only after maintainer review.
Then run the normal ROM build. For a focused validation first:

  m PeridotEssential peridot_daemon peridot_ctl selinux_policy
EOF
