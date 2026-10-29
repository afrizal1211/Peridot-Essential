#!/usr/bin/env bash
set -euo pipefail

PERIDOT_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PERIDOT_AOSP="$PERIDOT_PROJECT_ROOT/android/aosp"
PERIDOT_PRODUCT_PRIVATE="$PERIDOT_AOSP/sepolicy/product/private"
PERIDOT_VENDOR_POLICY="$PERIDOT_AOSP/sepolicy/vendor/peridot_daemon.te"
PERIDOT_GENFS="$PERIDOT_AOSP/sepolicy/vendor/genfs_contexts"

for PERIDOT_REQUIRED in \
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
    [ -s "$PERIDOT_AOSP/$PERIDOT_REQUIRED" ] || {
        echo "Missing AOSP integration file: $PERIDOT_REQUIRED" >&2
        exit 1
    }
done

grep -Fq 'presigned: true' "$PERIDOT_AOSP/Android.bp"
grep -Fq 'preprocessed: true' "$PERIDOT_AOSP/Android.bp"
grep -Fq 'dex_preopt:' "$PERIDOT_AOSP/Android.bp"
grep -Fq '"libbinder_ndk"' "$PERIDOT_AOSP/Android.bp"
if grep -Eq 'certificate:[[:space:]]*"platform"|check_elf_files:[[:space:]]*false' \
    "$PERIDOT_AOSP/Android.bp"; then
    echo "AOSP prebuilts must retain their signing identity and ELF checks" >&2
    exit 1
fi

grep -Fq 'PRODUCT_PACKAGES_DEBUG +=' "$PERIDOT_AOSP/peridot-essential.mk"
if sed -n '/^PRODUCT_PACKAGES +=/,/^$/p' "$PERIDOT_AOSP/peridot-essential.mk" \
    | grep -Fq 'peridot_ctl'; then
    echo "peridot_ctl must not be installed in production user builds" >&2
    exit 1
fi

grep -Fxq '[@PERIDOT_ESSENTIAL]' "$PERIDOT_PRODUCT_PRIVATE/keys.conf"
grep -Fq 'vendor/peridot-essential/prebuilt/PeridotEssential.x509.pem' \
    "$PERIDOT_PRODUCT_PRIVATE/keys.conf"
grep -Fq '<signer signature="@PERIDOT_ESSENTIAL">' \
    "$PERIDOT_PRODUCT_PRIVATE/mac_permissions.xml"
grep -Fq '<package name="io.github.davvv98.peridotessential">' \
    "$PERIDOT_PRODUCT_PRIVATE/mac_permissions.xml"
grep -Fq '<seinfo value="peridot_essential"' \
    "$PERIDOT_PRODUCT_PRIVATE/mac_permissions.xml"
python3 - "$PERIDOT_PRODUCT_PRIVATE/mac_permissions.xml" <<'PY'
import sys
import xml.etree.ElementTree as ET

root = ET.parse(sys.argv[1]).getroot()
if root.tag != "policy":
    raise SystemExit("mac_permissions.xml root must be <policy>")
PY

grep -Fxq \
    'user=_app seinfo=peridot_essential name=io.github.davvv98.peridotessential domain=peridot_app type=app_data_file levelFrom=all' \
    "$PERIDOT_PRODUCT_PRIVATE/seapp_contexts"
grep -Fq 'untrusted_app_domain(peridot_app)' "$PERIDOT_PRODUCT_PRIVATE/peridot_app.te"
grep -Fq 'net_domain(peridot_app)' "$PERIDOT_PRODUCT_PRIVATE/peridot_app.te"
grep -Fq 'typeattribute peridot_app coredomain;' "$PERIDOT_PRODUCT_PRIVATE/peridot_app.te"
grep -Fq 'allow peridot_app peridot_daemon:unix_stream_socket connectto;' \
    "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'typeattribute peridot_daemon mlstrustedsubject;' \
    "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'settings_service:service_manager find;' \
    "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'window_service:service_manager find;' \
    "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'activity_service:service_manager find;' \
    "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'overlay_service:service_manager find;' \
    "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'allow system_server peridot_daemon:fd use;' \
    "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'allow system_server peridot_daemon:fifo_file write;' \
    "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'toolbox_exec:file' "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'execute_no_trans' "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
if grep -Fq 'shell_exec:file' "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"; then
    echo "AOSP daemon must call cmd directly instead of executing shell wrappers" >&2
    exit 1
fi
if grep -Eq 'allow[[:space:]]+(appdomain|untrusted_app)[[:space:]]+peridot_daemon' \
    "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"; then
    echo "AOSP socket access must remain restricted to peridot_app" >&2
    exit 1
fi

if grep -Eq '(^|[[:space:]])permissive[[:space:]]+peridot_daemon|allow[[:space:]]+peridot_daemon[[:space:]]+sysfs:file|ctl_(start|stop)_prop' \
    "$PERIDOT_AOSP"/sepolicy/{product/private,product/public,vendor}/*; then
    echo "AOSP policy contains an unsafe or obsolete grant" >&2
    exit 1
fi

# Traversal is required on the reported vendor base; broader parent access is not.
grep -Fxq 'allow peridot_daemon sysfs_batteryinfo:dir search;' "$PERIDOT_VENDOR_POLICY"
if grep '^[[:space:]]*allow .*sysfs_batteryinfo' "$PERIDOT_VENDOR_POLICY" \
    | grep -Fvx 'allow peridot_daemon sysfs_batteryinfo:dir search;'; then
    echo "Battery parent access must remain directory-search only" >&2
    exit 1
fi
grep -Fxq 'allow peridot_daemon sysfs_battery_supply:dir search;' "$PERIDOT_VENDOR_POLICY"
grep -Fxq 'allow peridot_daemon sysfs_usb_supply:dir search;' "$PERIDOT_VENDOR_POLICY"

# Installed-app discovery now needs lookup, not package mutation privileges.
grep -Fxq 'allow peridot_daemon package_service:service_manager find;' "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fxq 'read_logd(peridot_daemon)' "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
if grep 'package_service' "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te" | grep -Fvx 'allow peridot_daemon package_service:service_manager find;'; then
    echo "Package discovery must use only the scoped service lookup grant" >&2
    exit 1
fi

grep -Fq 'type sysfs_peridot_mali_devfreq, fs_type, sysfs_type;' "$PERIDOT_VENDOR_POLICY"
grep -Fq 'type sysfs_peridot_touch, fs_type, sysfs_type;' "$PERIDOT_VENDOR_POLICY"
grep -Fq 'allow peridot_daemon sysfs_battery_supply:file rw_file_perms;' "$PERIDOT_VENDOR_POLICY"
grep -Fq 'netlink_kobject_uevent_socket' "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq 'allow peridot_daemon vendor_sysfs_displayfeature:file r_file_perms;' "$PERIDOT_VENDOR_POLICY"
grep -Fq 'allow peridot_daemon input_device:chr_file rw_file_perms;' "$PERIDOT_VENDOR_POLICY"
grep -Fq 'binder_call(peridot_daemon, hal_touchfeature_xiaomi_default)' "$PERIDOT_VENDOR_POLICY"
grep -Fq 'binder_call(peridot_daemon, vendor_hal_displayfeature_xiaomi_default)' \
    "$PERIDOT_VENDOR_POLICY"
grep -Fq 'allow peridot_daemon hal_touchfeature_xiaomi_service:service_manager find;' \
    "$PERIDOT_VENDOR_POLICY"
grep -Fq 'allow peridot_daemon hal_touchfeature_xiaomi_default:process ptrace;' "$PERIDOT_VENDOR_POLICY"
grep -Fq 'allow peridot_daemon hal_touchfeature_xiaomi_default:dir search;' "$PERIDOT_VENDOR_POLICY"
grep -Fq 'typeattribute peridot_daemon peridot_native_touch_access;' "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"
grep -Fq '/devices/platform/soc/13000000.mali/devfreq/13000000.mali' "$PERIDOT_GENFS"
grep -Fq '/devices/platform/goodix_ts.0' "$PERIDOT_GENFS"

grep -Fq 'user root' "$PERIDOT_AOSP/peridot_daemon.rc"
grep -Fq 'setenv PERIDOT_STATE_DIR /data/system/peridot-essential' "$PERIDOT_AOSP/peridot_daemon.rc"
grep -Fq 'setenv PERIDOT_TOUCH_DIRECT_INPUT 1' "$PERIDOT_AOSP/peridot_daemon.rc"
grep -Eq '^[[:space:]]*group.*input' "$PERIDOT_AOSP/peridot_daemon.rc"
grep -Fq 'capabilities DAC_OVERRIDE SYS_ADMIN SYS_NICE SYS_PTRACE' \
    "$PERIDOT_AOSP/peridot_daemon.rc"
if grep -Eq 'DAC_READ_SEARCH|readproc' \
    "$PERIDOT_AOSP/peridot_daemon.rc" "$PERIDOT_PRODUCT_PRIVATE/peridot_daemon.te"; then
    echo "AOSP daemon requests an unreviewed capability or process group" >&2
    exit 1
fi
grep -Fq 'mod touch_resampler;' "$PERIDOT_PROJECT_ROOT/runtime/daemon-rust/src/lib.rs"
grep -Fq 'PERIDOT_TOUCH_DIRECT_INPUT' \
    "$PERIDOT_PROJECT_ROOT/runtime/daemon-rust/src/touch_resampler.rs"
grep -Fq 'on property:sys.boot_completed=1' "$PERIDOT_AOSP/peridot_daemon.rc"

PERIDOT_PLAT_CIL="${PERIDOT_PLAT_SEPOLICY_CIL:-}"
if [ -n "$PERIDOT_PLAT_CIL" ]; then
    [ -f "$PERIDOT_PLAT_CIL" ] || {
        echo "PERIDOT_PLAT_SEPOLICY_CIL does not exist: $PERIDOT_PLAT_CIL" >&2
        exit 1
    }
    command -v secilc >/dev/null 2>&1 || {
        echo "secilc is required for the requested platform neverallow check" >&2
        exit 1
    }
    PERIDOT_CONTRACT_CIL="$PERIDOT_PROJECT_ROOT/tools/aosp-policy-contract.cil"
    PERIDOT_POLICY_TMP="$(mktemp -d)"
    trap 'rm -rf "$PERIDOT_POLICY_TMP"' EXIT

    # An incomplete or failing baseline is not a successful policy check.
    # Never disable neverallows or excuse unknown diagnostics as OEM noise.
    secilc -m -M true -G -c 30 "$PERIDOT_PLAT_CIL" \
        -o "$PERIDOT_POLICY_TMP/baseline-policy" -f /dev/null
    secilc -m -M true -G -c 30 \
        "$PERIDOT_PLAT_CIL" "$PERIDOT_CONTRACT_CIL" \
        -o "$PERIDOT_POLICY_TMP/combined-policy" -f /dev/null
    echo 'AOSP_PLATFORM_NEVERALLOW_TEST=PASS (synthetic probe only)'
else
    echo 'AOSP_PLATFORM_NEVERALLOW_TEST=SKIP (set PERIDOT_PLAT_SEPOLICY_CIL)'
fi

echo 'AOSP_INTEGRATION_CONTRACT_TEST=PASS'
