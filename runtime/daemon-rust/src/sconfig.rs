//! Explicit Xiaomi thermal scenario selection, independent of CPU/GPU profiles.
use super::{mutate_persisted_state, service_control};
use std::{fs, path::PathBuf, sync::Mutex};
const JOURNAL: &str = "peridot.sconfig.original";
const PATHS: [&str; 2] = [
    "/sys/class/thermal/thermal_message/sconfig",
    "/sys/devices/virtual/thermal/thermal_message/sconfig",
];
static LOCK: Mutex<()> = Mutex::new(());
pub(super) fn supported_mode(mode: i32) -> bool {
    matches!(mode, 0 | 6 | 18 | 50)
}
fn path() -> Result<PathBuf, String> {
    #[cfg(not(target_os = "android"))]
    if let Some(value) = std::env::var_os("PERIDOT_TEST_SCONFIG_PATH") {
        return Ok(value.into());
    }
    PATHS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .ok_or_else(|| "Xiaomi Sconfig node unavailable".into())
}
fn parse(raw: &str) -> Result<i32, String> {
    let value = raw
        .trim()
        .parse::<i32>()
        .map_err(|e| format!("Sconfig parse: {e}"))?;
    if (-1..=2048).contains(&value) {
        Ok(value)
    } else {
        Err(format!("invalid Sconfig readback {value}"))
    }
}
pub(super) fn read() -> Result<i32, String> {
    parse(&fs::read_to_string(path()?).map_err(|e| format!("Sconfig read: {e}"))?)
}
pub(super) fn supported() -> bool {
    read().is_ok()
}
pub(super) fn original() -> Option<i32> {
    service_control::original(JOURNAL)?
        .as_i64()
        .and_then(|v| i32::try_from(v).ok())
}
fn write(mode: i32) -> Result<(), String> {
    fs::write(path()?, mode.to_string()).map_err(|e| format!("Sconfig write {mode}: {e}"))?;
    let actual = read()?;
    if actual != mode {
        return Err(format!("Sconfig verify requested={mode} actual={actual}"));
    }
    Ok(())
}
fn rollback(error: String, previous: i32) -> String {
    match write(previous) {
        Ok(()) => error,
        Err(e) => format!("{error}; rollback incomplete: {e}"),
    }
}
pub(super) fn set(mode: i32) -> Result<(), String> {
    if mode != -1 && !supported_mode(mode) {
        return Err(format!("unsupported Sconfig selection {mode}"));
    }
    let _lock = LOCK.lock().map_err(|_| "Sconfig lock poisoned")?;
    if mode == -1 && original().is_none() {
        return mutate_persisted_state(|state| state.sconfig_override = -1);
    }
    let previous = read()?;
    if mode >= 0 {
        service_control::remember(JOURNAL, serde_json::json!(previous))?;
    }
    let target = if mode == -1 {
        original().ok_or("Sconfig original unavailable")?
    } else {
        mode
    };
    if let Err(e) = write(target) {
        return Err(rollback(e, previous));
    }
    if let Err(e) = mutate_persisted_state(|state| state.sconfig_override = mode) {
        return Err(rollback(e, previous));
    }
    if mode == -1 {
        service_control::forget(&[JOURNAL.into()])?;
    }
    Ok(())
}
/// Release hardware ownership, keeping desired selection for enable/resume.
pub(super) fn release() -> Result<(), String> {
    let _lock = LOCK.lock().map_err(|_| "Sconfig lock poisoned")?;
    if let Some(value) = original() {
        write(value)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_exposed_stock_scenarios_can_be_selected() {
        for mode in [0, 6, 18, 50] {
            assert!(supported_mode(mode));
        }
        for mode in [-1, 1, 7, 2048, 9999] {
            assert!(!supported_mode(mode));
        }
    }
    #[test]
    fn original_automatic_state_is_preserved_without_guessing_zero() {
        assert_eq!(parse("-1\n").unwrap(), -1);
        assert_eq!(parse("6\n").unwrap(), 6);
        assert!(parse("6 extra").is_err());
        assert!(parse("4096").is_err());
    }
}
