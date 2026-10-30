//! Read-only probe of the POCO F6 (`peridot`) hardware interfaces that the
//! peridot-dev LineageOS 23.2 trees document. Nothing here writes hardware.
//!
//! The device tree ships `vendor.lineage.touch-service.xiaomi` instead of the
//! Xiaomi TouchFeature AIDL service, UFS is `ufshc@1d84000`, memory scaling is
//! QCOM DCVS rather than devfreq, and fast charging is exposed through
//! `/sys/class/qcom-battery/fastcharge_enable`. The probe reports what this
//! kernel actually exposes so the UI never advertises an absent path.
use super::{PROTOCOL_VERSION, read_trimmed, ufs_scheduler_paths, vendor_binder};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

/// Capacities, monitors and paths are exercised through the rail kobjects.
pub const DCVS_BASE: &str = "/sys/devices/system/cpu/bus_dcvs";
/// `dcvs.c` creates one kobject per `qcom,dcvs-hw` child; `cliffs.dtsi` defines
/// exactly these four for SM8635.
pub const RAILS: [&str; 4] = ["ddr", "l3", "llcc", "ddrqos"];
/// `cliffs.dtsi`: `ufshc_mem: ufshc@1d84000`.
pub const UFS_DEVFREQ_PATH: &str = "/sys/class/devfreq/1d84000.ufshc";
pub const FASTCHARGE_PATH: &str = "/sys/class/qcom-battery/fastcharge_enable";
/// Highest value accepted by `fastcharge_enable_store` in qti_battery_charger.c.
pub const FASTCHARGE_MAX: i32 = 2;
pub const HTPR_PATHS: [&str; 2] = [
    "/sys/devices/virtual/touch/touch_dev/bump_sample_rate",
    "/sys/class/touch/touch_dev/bump_sample_rate",
];
pub const DISPLAY_SYSFS: &str = "/sys/class/mi_display/disp-DSI-0";
pub const XIAOMI_TOUCH_DEV: &str = "/dev/xiaomi-touch";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TouchBackend {
    /// Neither vendor interface is exposed.
    None,
    /// Xiaomi `vendor.xiaomi.hw.touchfeature.ITouchFeature` AIDL service.
    TouchFeature,
    /// Lineage `vendor.lineage.touch-service.xiaomi` HTPR sysfs toggle.
    Htpr,
}

impl TouchBackend {
    pub fn code(self) -> i32 {
        match self {
            Self::None => 0,
            Self::TouchFeature => 1,
            Self::Htpr => 2,
        }
    }
}

/// The HTPR node is a boolean high-polling toggle, not a frequency selector.
pub fn htpr_path() -> Option<&'static str> {
    HTPR_PATHS
        .into_iter()
        .find(|path| Path::new(path).is_file())
}

pub fn touch_backend() -> TouchBackend {
    if vendor_binder::touch_available() {
        TouchBackend::TouchFeature
    } else if htpr_path().is_some() {
        TouchBackend::Htpr
    } else {
        TouchBackend::None
    }
}

fn number(path: &Path) -> Option<u64> {
    read_trimmed(path).ok()?.parse().ok()
}

fn frequencies(path: &Path) -> Vec<u64> {
    let mut values = read_trimmed(path)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|value| value.parse::<u64>().ok())
        .collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    values
}

/// UFS devfreq device: prefer the documented address, then any `ufshc` node so
/// a kernel that renames the controller still reports correctly.
pub fn ufs_devfreq() -> Option<PathBuf> {
    let documented = Path::new(UFS_DEVFREQ_PATH);
    if documented.join("available_frequencies").is_file() {
        return Some(documented.to_path_buf());
    }
    fs::read_dir("/sys/class/devfreq")
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().contains("ufshc"))
                .unwrap_or(false)
                && path.join("available_frequencies").is_file()
        })
}

/// One DCVS rail. `boost_freq` is a floor vote; the driver clamps it to
/// `[hw_min_freq, hw_max_freq]` and rejects anything above `hw_max_freq`.
pub fn dcvs_rail(root: &Path) -> Value {
    let supported =
        root.join("boost_freq").is_file() && root.join("hw_max_freq").is_file();
    json!({
        "supported": supported,
        "boost_hz": number(&root.join("boost_freq")),
        "hw_min_hz": number(&root.join("hw_min_freq")),
        "hw_max_hz": number(&root.join("hw_max_freq")),
        "cur_hz": number(&root.join("cur_freq")),
        "frequencies_hz": frequencies(&root.join("available_frequencies")),
    })
}

pub fn fastcharge_status(path: &Path) -> Value {
    json!({
        "supported": path.is_file(),
        "value": read_trimmed(path).ok().and_then(|value| value.parse::<i32>().ok()),
        "max": FASTCHARGE_MAX,
    })
}

pub fn probe_touch() -> Value {
    let backend = touch_backend();
    json!({
        "backend": match backend {
            TouchBackend::None => "none",
            TouchBackend::TouchFeature => "xiaomi_touchfeature_aidl",
            TouchBackend::Htpr => "lineage_htpr",
        },
        "touchfeature": vendor_binder::touch_available(),
        "htpr_path": htpr_path(),
        "htpr_candidates": HTPR_PATHS,
        "xiaomi_touch_dev": Path::new(XIAOMI_TOUCH_DEV).exists(),
    })
}

pub fn probe_storage() -> Value {
    let devfreq = ufs_devfreq();
    let status = devfreq.as_deref().map(|root| {
        json!({
            "name": read_trimmed(root.join("name")).ok(),
            "available_frequencies": frequencies(&root.join("available_frequencies")),
            "governor": read_trimmed(root.join("governor")).ok(),
            "min_hz": number(&root.join("min_freq")),
            "max_hz": number(&root.join("max_freq")),
            "cur_hz": number(&root.join("cur_freq")),
        })
    });
    json!({
        "devfreq_path": devfreq.map(|path| path.to_string_lossy().into_owned()),
        "devfreq": status,
        "scheduler_paths": ufs_scheduler_paths()
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
    })
}

pub fn probe_memory() -> Value {
    let rails = RAILS
        .into_iter()
        .map(|rail| (rail.to_string(), dcvs_rail(&Path::new(DCVS_BASE).join(rail))))
        .collect::<serde_json::Map<String, Value>>();
    json!({"base": DCVS_BASE, "rails": rails})
}

pub fn probe_charging() -> Value {
    json!({
        "fastcharge": fastcharge_status(Path::new(FASTCHARGE_PATH)),
        "component": Path::new("/sys/class/power_supply/battery").exists()
            && Path::new("/sys/class/power_supply/usb").exists(),
        "bypass": Path::new("/sys/class/power_supply/battery/bypass_charging").exists()
            || Path::new("/sys/class/power_supply/battery/bypass_charge").exists(),
        "bypass_supported": Path::new(
            "/sys/class/power_supply/battery/bypass_charging_supported"
        )
        .exists(),
    })
}

pub fn probe_display() -> Value {
    json!({
        "aidl": vendor_binder::display_available(),
        "sysfs": Path::new(DISPLAY_SYSFS).exists(),
    })
}

/// Full read-only contract, returned by `GET hardware.contract`.
pub fn probe() -> Value {
    json!({
        "protocol": PROTOCOL_VERSION,
        "device": "peridot",
        "soc": "SM8635",
        "touch": probe_touch(),
        "storage": probe_storage(),
        "memory": probe_memory(),
        "charging": probe_charging(),
        "display": probe_display(),
    })
}

/// Bitmask used by the Android snapshot: ddr=1, l3=2, llcc=4, ddrqos=8.
pub fn dcvs_mask() -> i32 {
    RAILS
        .into_iter()
        .enumerate()
        .filter(|(_, rail)| Path::new(DCVS_BASE).join(rail).join("boost_freq").is_file())
        .map(|(index, _)| 1 << index)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rail_dir(name: &str, boost: &str, min: &str, max: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("peridot-rail-{}-{name}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("boost_freq"), boost).unwrap();
        fs::write(root.join("hw_min_freq"), min).unwrap();
        fs::write(root.join("hw_max_freq"), max).unwrap();
        fs::write(root.join("cur_freq"), "1555000").unwrap();
        fs::write(root.join("available_frequencies"), "547000 1555000 4224000").unwrap();
        root
    }

    #[test]
    fn dcvs_rail_reports_floor_and_table() {
        let root = rail_dir("ddr", "547000", "547000", "4224000");
        let status = dcvs_rail(&root);
        assert_eq!(status["supported"], true);
        assert_eq!(status["boost_hz"], 547000);
        assert_eq!(status["hw_max_hz"], 4224000);
        assert_eq!(status["frequencies_hz"].as_array().unwrap().len(), 3);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dcvs_rail_without_boost_is_unsupported() {
        let root = std::env::temp_dir().join(format!("peridot-rail-none-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("hw_max_freq"), "4224000").unwrap();
        assert_eq!(dcvs_rail(&root)["supported"], false);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fastcharge_status_tracks_node_and_value() {
        let path = std::env::temp_dir().join(format!("peridot-fastcharge-{}", std::process::id()));
        fs::write(&path, "1\n").unwrap();
        let status = fastcharge_status(&path);
        assert_eq!(status["supported"], true);
        assert_eq!(status["value"], 1);
        assert_eq!(status["max"], 2);
        fs::remove_file(path).unwrap();
    }
}
