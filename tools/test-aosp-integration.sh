#!/usr/bin/env bash
set -euo pipefail

ONYX_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ONYX_AOSP="$ONYX_PROJECT_ROOT/android/aosp"
ONYX_PRODUCT_PRIVATE="$ONYX_AOSP/sepolicy/product/private"
ONYX_VENDOR_POLICY="$ONYX_AOSP/sepolicy/vendor/peridot_daemon.te"
ONYX_GENFS="$ONYX_AOSP/sepolicy/vendor/genfs_contexts"

for ONYX_REQUIRED in \
    Android.bp BoardConfigPeridotEssential.mk peridot-essential.mk peridot_daemon.rc \
    sepolicy/product/public/peridot_daemon.te \
    sepolicy/product/private/file_contexts \
    sepolicy/product/private/keys.conf \
    sepolicy/product/private/mac_permissions.xml \
    sepolicy/product/private/peridot_app.te \
    sepolicy/product/private/peridot_daemon.te \
    sepolicy/product/private/seapp_contexts \
    sepolicy/vendor/genfs_contexts \
    sepolicy/vendor/peridot_daemon.te; do
    [ -s "$ONYX_AOSP/$ONYX_REQUIRED" ] || {
        echo "Missing AOSP integration file: $ONYX_REQUIRED" >&2
        exit 1
    }
done

grep -Fq 'presigned: true' "$ONYX_AOSP/Android.bp"
grep -Fq 'preprocessed: true' "$ONYX_AOSP/Android.bp"
grep -Fq 'dex_preopt:' "$ONYX_AOSP/Android.bp"
grep -Fq '"libbinder_ndk"' "$ONYX_AOSP/Android.bp"
if grep -Eq 'certificate:[[:space:]]*"platform"|check_elf_files:[[:space:]]*false' \
    "$ONYX_AOSP/Android.bp"; then
    echo "AOSP prebuilts must retain their signing identity and ELF checks" >&2
    exit 1
fi

grep -Fq 'PRODUCT_PACKAGES_DEBUG +=' "$ONYX_AOSP/peridot-essential.mk"
if sed -n '/^PRODUCT_PACKAGES +=/,/^$/p' "$ONYX_AOSP/peridot-essential.mk" \
    | grep -Fq 'peridot_ctl'; then
    echo "peridot_ctl must not be installed in production user builds" >&2
    exit 1
fi

grep -Fxq '[@ONYX_ESSENTIAL]' "$ONYX_PRODUCT_PRIVATE/keys.conf"
grep -Fq 'vendor/peridot-essential/prebuilt/PeridotEssential.x509.pem' \
    "$ONYX_PRODUCT_PRIVATE/keys.conf"
grep -Fq '<signer signature="@ONYX_ESSENTIAL">' \
    "$ONYX_PRODUCT_PRIVATE/mac_permissions.xml"
grep -Fq '<package name="io.github.davvv98.peridotessential">' \
    "$ONYX_PRODUCT_PRIVATE/mac_permissions.xml"
grep -Fq '<seinfo value="peridot_essential"' \
    "$ONYX_PRODUCT_PRIVATE/mac_permissions.xml"
python3 - "$ONYX_PRODUCT_PRIVATE/mac_permissions.xml" <<'PY'
import sys
import xml.etree.ElementTree as ET

root = ET.parse(sys.argv[1]).getroot()
if root.tag != "policy":
    raise SystemExit("mac_permissions.xml root must be <policy>")
PY

grep -Fxq \
    'user=_app seinfo=peridot_essential name=io.github.davvv98.peridotessential domain=peridot_app type=app_data_file levelFrom=all' \
    "$ONYX_PRODUCT_PRIVATE/seapp_contexts"
grep -Fq 'untrusted_app_domain(peridot_app)' "$ONYX_PRODUCT_PRIVATE/peridot_app.te"
grep -Fq 'net_domain(peridot_app)' "$ONYX_PRODUCT_PRIVATE/peridot_app.te"
grep -Fq 'typeattribute peridot_app coredomain;' "$ONYX_PRODUCT_PRIVATE/peridot_app.te"
grep -Fq 'allow peridot_app peridot_daemon:unix_stream_socket connectto;' \
    "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'typeattribute peridot_daemon mlstrustedsubject;' \
    "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'settings_service:service_manager find;' \
    "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'window_service:service_manager find;' \
    "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'activity_service:service_manager find;' \
    "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'overlay_service:service_manager find;' \
    "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'allow system_server peridot_daemon:fd use;' \
    "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'allow system_server peridot_daemon:fifo_file write;' \
    "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'toolbox_exec:file' "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'execute_no_trans' "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
if grep -Fq 'shell_exec:file' "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"; then
    echo "AOSP daemon must call cmd directly instead of executing shell wrappers" >&2
    exit 1
fi
if grep -Eq 'allow[[:space:]]+(appdomain|untrusted_app)[[:space:]]+peridot_daemon' \
    "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"; then
    echo "AOSP socket access must remain restricted to peridot_app" >&2
    exit 1
fi

if grep -Eq '(^|[[:space:]])permissive[[:space:]]+peridot_daemon|allow[[:space:]]+peridot_daemon[[:space:]]+sysfs:file|ctl_(start|stop)_prop' \
    "$ONYX_AOSP"/sepolicy/{product/private,product/public,vendor}/*; then
    echo "AOSP policy contains an unsafe or obsolete grant" >&2
    exit 1
fi

# Traversal is required on the reported vendor base; broader parent access is not.
grep -Fxq 'allow peridot_daemon sysfs_batteryinfo:dir search;' "$ONYX_VENDOR_POLICY"
if grep '^[[:space:]]*allow .*sysfs_batteryinfo' "$ONYX_VENDOR_POLICY" \
    | grep -Fvx 'allow peridot_daemon sysfs_batteryinfo:dir search;'; then
    echo "Battery parent access must remain directory-search only" >&2
    exit 1
fi
grep -Fxq 'allow peridot_daemon sysfs_battery_supply:dir search;' "$ONYX_VENDOR_POLICY"
grep -Fxq 'allow peridot_daemon sysfs_usb_supply:dir search;' "$ONYX_VENDOR_POLICY"

# Installed-app discovery now needs lookup, not package mutation privileges.
grep -Fxq 'allow peridot_daemon package_service:service_manager find;' "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fxq 'read_logd(peridot_daemon)' "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
if grep 'package_service' "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te" | grep -Fvx 'allow peridot_daemon package_service:service_manager find;'; then
    echo "Package discovery must use only the scoped service lookup grant" >&2
    exit 1
fi

grep -Fq 'type sysfs_peridot_mali_devfreq, fs_type, sysfs_type;' "$ONYX_VENDOR_POLICY"
grep -Fq 'type sysfs_peridot_touch, fs_type, sysfs_type;' "$ONYX_VENDOR_POLICY"
grep -Fq 'allow peridot_daemon sysfs_battery_supply:file rw_file_perms;' "$ONYX_VENDOR_POLICY"
grep -Fq 'netlink_kobject_uevent_socket' "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'allow peridot_daemon vendor_sysfs_displayfeature:file r_file_perms;' "$ONYX_VENDOR_POLICY"
grep -Fq 'allow peridot_daemon input_device:chr_file rw_file_perms;' "$ONYX_VENDOR_POLICY"
grep -Fq 'binder_call(peridot_daemon, hal_touchfeature_xiaomi_default)' "$ONYX_VENDOR_POLICY"
grep -Fq 'binder_call(peridot_daemon, vendor_hal_displayfeature_xiaomi_default)' \
    "$ONYX_VENDOR_POLICY"
grep -Fq 'allow peridot_daemon hal_touchfeature_xiaomi_service:service_manager find;' \
    "$ONYX_VENDOR_POLICY"
grep -Fq 'allow peridot_daemon hal_touchfeature_xiaomi_default:process ptrace;' "$ONYX_VENDOR_POLICY"
grep -Fq 'allow peridot_daemon hal_touchfeature_xiaomi_default:dir search;' "$ONYX_VENDOR_POLICY"
grep -Fq 'typeattribute peridot_daemon peridot_native_touch_access;' "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq '/devices/platform/soc/13000000.mali/devfreq/13000000.mali' "$ONYX_GENFS"
grep -Fq '/devices/platform/goodix_ts.0' "$ONYX_GENFS"

grep -Fq 'user root' "$ONYX_AOSP/peridot_daemon.rc"
grep -Fq 'setenv ONYX_STATE_DIR /data/system/peridot-essential' "$ONYX_AOSP/peridot_daemon.rc"
grep -Fq 'setenv ONYX_TOUCH_DIRECT_INPUT 1' "$ONYX_AOSP/peridot_daemon.rc"
grep -Eq '^[[:space:]]*group.*input' "$ONYX_AOSP/peridot_daemon.rc"
grep -Fq 'capabilities DAC_OVERRIDE SYS_ADMIN SYS_NICE SYS_PTRACE' \
    "$ONYX_AOSP/peridot_daemon.rc"
if grep -Eq 'DAC_READ_SEARCH|readproc' \
    "$ONYX_AOSP/peridot_daemon.rc" "$ONYX_PRODUCT_PRIVATE/peridot_daemon.te"; then
    echo "AOSP daemon requests an unreviewed capability or process group" >&2
    exit 1
fi
grep -Fq 'mod touch_resampler;' "$ONYX_PROJECT_ROOT/runtime/daemon-rust/src/lib.rs"
grep -Fq 'ONYX_TOUCH_DIRECT_INPUT' \
    "$ONYX_PROJECT_ROOT/runtime/daemon-rust/src/touch_resampler.rs"
grep -Fq 'on property:sys.boot_completed=1' "$ONYX_AOSP/peridot_daemon.rc"

ONYX_PLAT_CIL="${ONYX_PLAT_SEPOLICY_CIL:-}"
if [ -n "$ONYX_PLAT_CIL" ]; then
    [ -f "$ONYX_PLAT_CIL" ] || {
        echo "ONYX_PLAT_SEPOLICY_CIL does not exist: $ONYX_PLAT_CIL" >&2
        exit 1
    }
    command -v secilc >/dev/null 2>&1 || {
        echo "secilc is required for the requested platform neverallow check" >&2
        exit 1
    }
    ONYX_CONTRACT_CIL="$ONYX_PROJECT_ROOT/tools/aosp-policy-contract.cil"
    ONYX_POLICY_TMP="$(mktemp -d)"
    trap 'rm -rf "$ONYX_POLICY_TMP"' EXIT

    # An incomplete or failing baseline is not a successful policy check.
    # Never disable neverallows or excuse unknown diagnostics as OEM noise.
    secilc -m -M true -G -c 30 "$ONYX_PLAT_CIL" \
        -o "$ONYX_POLICY_TMP/baseline-policy" -f /dev/null
    secilc -m -M true -G -c 30 \
        "$ONYX_PLAT_CIL" "$ONYX_CONTRACT_CIL" \
        -o "$ONYX_POLICY_TMP/combined-policy" -f /dev/null
    echo 'AOSP_PLATFORM_NEVERALLOW_TEST=PASS (synthetic probe only)'
else
    echo 'AOSP_PLATFORM_NEVERALLOW_TEST=SKIP (set ONYX_PLAT_SEPOLICY_CIL)'
fi

echo 'AOSP_INTEGRATION_CONTRACT_TEST=PASS'
