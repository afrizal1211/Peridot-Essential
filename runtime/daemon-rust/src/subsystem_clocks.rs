//! NEESCHAL: a memory/storage request is not a licence to poke raw PLLs.
//!
//! Storage uses the UFS devfreq node (`ufshc@1d84000` on peridot) with its
//! supported `min_freq`/`max_freq` bounds. SM8635 memory scaling is QCOM DCVS,
//! not devfreq: each rail under `/sys/devices/system/cpu/bus_dcvs` exposes a
//! writable `boost_freq` floor and read-only `hw_min_freq`, `hw_max_freq`,
//! `cur_freq` and `available_frequencies`. The floor is a vote, not a cap.
//!
//! No saved maximum or floor is replayed at boot. A write-ahead original is
//! recovered on daemon restart; a kernel hang still requires external recovery.
use super::*;
use hardware_contract::DCVS_BASE;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const TRIAL_SECONDS: u64 = 30;
const RAILS: [&str; 4] = ["ddr", "l3", "llcc", "ddrqos"];

#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Memory,
    Storage,
}
impl Kind {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "memory" => Ok(Self::Memory),
            "storage" => Ok(Self::Storage),
            _ => Err("unknown subsystem".into()),
        }
    }
    fn storage_root() -> PathBuf {
        hardware_contract::ufs_devfreq()
            .unwrap_or_else(|| PathBuf::from(hardware_contract::UFS_DEVFREQ_PATH))
    }
}

#[derive(Default)]
struct Trials {
    /// Devfreq range journal for the storage controller.
    storage: Option<Original>,
    /// DCVS `boost_freq` originals keyed by rail.
    rails: BTreeMap<String, u64>,
    storage_deadline: Option<Instant>,
    rail_deadlines: BTreeMap<String, Instant>,
    storage_target: Option<(u64, u64)>,
    rail_targets: BTreeMap<String, u64>,
    error: Option<String>,
    journal_invalid: bool,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
enum Original {
    Legacy(u64),
    Range { min_hz: u64, max_hz: u64 },
}

/// On-disk recovery journal. Legacy `memory` devfreq records are accepted and
/// dropped: the node they referenced does not exist on SM8635.
#[derive(Default, Serialize, Deserialize)]
struct Journal {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    storage: Option<Original>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    rails: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory: Option<Original>,
}

static TRIALS: OnceLock<(Mutex<Trials>, Condvar)> = OnceLock::new();
fn trials() -> &'static (Mutex<Trials>, Condvar) {
    TRIALS.get_or_init(|| (Mutex::new(Trials::default()), Condvar::new()))
}
fn number(root: &Path, name: &str) -> Result<u64, String> {
    fs::read_to_string(root.join(name))
        .map_err(|e| format!("{name}: {e}"))?
        .trim()
        .parse()
        .map_err(|_| format!("invalid {name}"))
}
fn table(raw: &str) -> Result<Vec<u64>, String> {
    let mut values = raw
        .split_whitespace()
        .map(|v| {
            v.parse::<u64>()
                .map_err(|_| "invalid frequency table".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    values.sort_unstable();
    values.dedup();
    if values.is_empty()
        || values.len() > 32
        || values.iter().any(|&v| v == 0 || v > 20_000_000_000)
    {
        return Err("unsupported frequency table".into());
    }
    Ok(values)
}
/// DCVS tables allow state `0` (ddrqos is a 0/1 toggle), so this variant keeps
/// zero while storage keeps rejecting it.
fn rail_table(raw: &str) -> Result<Vec<u64>, String> {
    let mut values = raw
        .split_whitespace()
        .map(|v| {
            v.parse::<u64>()
                .map_err(|_| "invalid frequency table".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    values.sort_unstable();
    values.dedup();
    if values.is_empty() || values.len() > 32 || values.iter().any(|&v| v > 20_000_000_000) {
        return Err("unsupported frequency table".into());
    }
    Ok(values)
}
fn rail_root(rail: &str) -> Result<PathBuf, String> {
    if !RAILS.contains(&rail) {
        return Err("unknown DCVS rail".into());
    }
    Ok(Path::new(DCVS_BASE).join(rail))
}
fn rail_ready(root: &Path) -> bool {
    root.join("boost_freq").is_file()
        && root.join("hw_min_freq").is_file()
        && root.join("hw_max_freq").is_file()
        && root.join("available_frequencies").is_file()
}
fn rail_status(rail: &str, root: &Path) -> Value {
    let ready = rail_ready(root);
    let mut item = hardware_contract::dcvs_rail(root);
    item["rail"] = json!(rail);
    item["supported"] = json!(ready);
    if !ready {
        item["reason"] = json!("this kernel does not expose DCVS boost control for the rail");
    }
    item
}
fn validate_boost(root: &Path, hz: u64) -> Result<(), String> {
    if !rail_ready(root) {
        return Err("this kernel does not expose DCVS boost control".into());
    }
    let min = number(root, "hw_min_freq")?;
    let max = number(root, "hw_max_freq")?;
    if hz < min || hz > max {
        return Err("This frequency is outside the driver hardware range".into());
    }
    let levels = rail_table(&read_trimmed(root.join("available_frequencies"))?)?;
    if !levels.contains(&hz) {
        return Err("Choose a frequency advertised by this driver".into());
    }
    Ok(())
}
fn write_boost(root: &Path, hz: u64) -> Result<(), String> {
    fs::write(root.join("boost_freq"), hz.to_string()).map_err(|e| e.to_string())?;
    if number(root, "boost_freq")? != hz {
        return Err("driver did not accept the requested floor".into());
    }
    Ok(())
}
fn write_floor(root: &Path, hz: u64) -> Result<(), String> {
    fs::write(root.join("min_freq"), hz.to_string()).map_err(|e| e.to_string())?;
    if number(root, "min_freq")? != hz {
        return Err("driver did not accept the requested floor".into());
    }
    Ok(())
}
fn write_range(root: &Path, min: u64, max: u64) -> Result<(), String> {
    if min > max {
        return Err("invalid frequency range".into());
    }
    // Keep the range valid at each step, including OEM restoration.
    if min > number(root, "max_freq")? {
        fs::write(root.join("max_freq"), max.to_string()).map_err(|e| e.to_string())?;
        write_floor(root, min)?;
    } else {
        write_floor(root, min)?;
        fs::write(root.join("max_freq"), max.to_string()).map_err(|e| e.to_string())?;
    }
    if number(root, "max_freq")? != max || number(root, "min_freq")? != min {
        return Err("driver did not accept the exact frequency range".into());
    }
    Ok(())
}
fn inspect_storage(root: &Path) -> Result<Value, String> {
    let name = fs::read_to_string(root.join("name")).map_err(|e| e.to_string())?;
    if !name.trim().contains("ufshc") {
        return Err("unexpected devfreq device".into());
    }
    let frequencies = table(
        &fs::read_to_string(root.join("available_frequencies")).map_err(|e| e.to_string())?,
    )?;
    let governor = fs::read_to_string(root.join("governor")).map_err(|e| e.to_string())?;
    let min = number(root, "min_freq")?;
    let max = number(root, "max_freq")?;
    Ok(json!({"supported": true, "frequencies_hz": frequencies,
        "driver_hz": number(root, "cur_freq").ok(),
        "min_hz": min, "max_hz": max, "governor": governor.trim()}))
}
fn inspect_storage_soft(root: &Path) -> Value {
    inspect_storage(root)
        .unwrap_or_else(|e| json!({"supported": false, "reason": e}))
}
fn apply_max(root: &Path) -> Result<u64, String> {
    let status = inspect_storage(root)?;
    let original = status["min_hz"].as_u64().ok_or("missing original floor")?;
    let max = status["frequencies_hz"]
        .as_array()
        .and_then(|a| a.last())
        .and_then(Value::as_u64)
        .ok_or("missing highest supported state")?;
    if max > status["max_hz"].as_u64().ok_or("missing ceiling")? {
        return Err("The existing driver ceiling excludes its highest state".into());
    }
    write_floor(root, max)?;
    Ok(original)
}
fn validate_range(root: &Path, min: u64, max: u64, ceiling: u64) -> Result<(), String> {
    let status = inspect_storage(root)?;
    let levels = status["frequencies_hz"]
        .as_array()
        .ok_or("missing supported states")?;
    if min > max
        || ![min, max]
            .iter()
            .all(|hz| levels.iter().any(|v| v.as_u64() == Some(*hz)))
    {
        return Err("Choose a frequency advertised by this driver".into());
    }
    if max > ceiling {
        return Err("This frequency exceeds the existing driver ceiling".into());
    }
    Ok(())
}
fn apply_supported_range(root: &Path, min: u64, max: u64, ceiling: u64) -> Result<(), String> {
    validate_range(root, min, max, ceiling)?;
    write_range(root, min, max)
}
#[cfg(test)]
fn apply_level(root: &Path, hz: u64, ceiling: u64) -> Result<(), String> {
    apply_supported_range(root, hz, hz, ceiling)
}
fn journal_path() -> PathBuf {
    state_dir().join("subsystem-clock-originals.json")
}
fn journal(state: &Trials) -> Result<(), String> {
    let path = journal_path();
    fs::create_dir_all(state_dir()).map_err(|e| e.to_string())?;
    let temp = path.with_extension("tmp");
    let record = Journal {
        storage: state.storage,
        rails: state.rails.clone(),
        memory: None,
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(
        serde_json::to_string(&record)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    fs::rename(&temp, &path).map_err(|e| e.to_string())?;
    fs::File::open(state_dir())
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
fn restore_storage(state: &mut Trials) -> Result<(), String> {
    let Some(original) = state.storage else {
        return Ok(());
    };
    let root = Kind::storage_root();
    let status = inspect_storage(&root)?;
    let values = status["frequencies_hz"].as_array().unwrap();
    let low = values
        .first()
        .and_then(Value::as_u64)
        .ok_or("missing lower state")?;
    let high = values
        .last()
        .and_then(Value::as_u64)
        .ok_or("missing upper state")?;
    match original {
        Original::Legacy(min_hz) => {
            let hz = min_hz.max(low).min(high);
            write_floor(&root, hz)?;
        }
        Original::Range { min_hz, max_hz } => {
            if !(low..=high).contains(&min_hz) || !(low..=high).contains(&max_hz) {
                return Err("original storage range is outside the supported request range".into());
            }
            write_range(&root, min_hz, max_hz)?;
        }
    }
    state.storage = None;
    state.storage_target = None;
    state.storage_deadline = None;
    journal(state)
}
fn restore_rail(state: &mut Trials, rail: &str) -> Result<(), String> {
    let Some(&original) = state.rails.get(rail) else {
        return Ok(());
    };
    let root = rail_root(rail)?;
    if !rail_ready(&root) {
        return Err(format!("DCVS rail {rail} is no longer available"));
    }
    let min = number(&root, "hw_min_freq")?;
    let max = number(&root, "hw_max_freq")?;
    // A kernel table can move between builds; never replay an out-of-range OPP.
    let hz = original.max(min).min(max);
    write_boost(&root, hz)?;
    let mut remaining = state.rails.clone();
    remaining.remove(rail);
    let mut next = Journal {
        storage: state.storage,
        rails: remaining,
        memory: None,
    };
    let path = journal_path();
    let temp = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(
        serde_json::to_string(&next)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    fs::rename(&temp, &path).map_err(|e| e.to_string())?;
    next.rails.clear();
    state.rails = next.rails;
    state.rail_targets.remove(rail);
    state.rail_deadlines.remove(rail);
    Ok(())
}
pub(super) fn restore_all() -> Result<(), String> {
    let mut state = trials().0.lock().map_err(|_| "clock trial lock poisoned")?;
    if state.journal_invalid {
        return Err("clock recovery journal is invalid; no hardware writes permitted".into());
    }
    restore_storage(&mut state)?;
    let rails = state.rails.keys().cloned().collect::<Vec<_>>();
    for rail in rails {
        restore_rail(&mut state, &rail)?;
    }
    state.error = None;
    trials().1.notify_one();
    Ok(())
}
pub(super) fn start() {
    let loaded = match fs::read(journal_path()) {
        Ok(bytes) => serde_json::from_slice::<Journal>(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Journal::default()),
        Err(e) => Err(e.to_string()),
    };
    {
        let mut state = trials().0.lock().unwrap();
        match loaded {
            Ok(record) => {
                // `record.memory` is a legacy devfreq record. Dropping it is
                // deliberate: that node never existed on SM8635, and replaying
                // a devfreq write against DCVS would be a different control.
                state.storage = record.storage;
                state.rails = record.rails;
            }
            Err(e) => {
                state.error = Some(e);
                state.journal_invalid = true;
            }
        }
    }
    if trials().0.lock().unwrap().error.is_none() {
        if let Err(e) = restore_all() {
            trials().0.lock().unwrap().error = Some(e);
        }
    }
    std::thread::spawn(|| {
        let (lock, wake) = trials();
        let mut state = lock.lock().unwrap();
        loop {
            let expired_storage = state
                .storage_deadline
                .is_some_and(|deadline| deadline <= Instant::now());
            let expired_rails = state
                .rail_deadlines
                .iter()
                .filter(|(_, deadline)| **deadline <= Instant::now())
                .map(|(rail, _)| rail.clone())
                .collect::<Vec<_>>();
            if expired_storage {
                if let Err(e) = restore_storage(&mut state) {
                    state.error = Some(e);
                }
            }
            for rail in expired_rails {
                if let Err(e) = restore_rail(&mut state, &rail) {
                    state.error = Some(e);
                    state.rail_deadlines.remove(&rail);
                }
            }
            let waits = state
                .storage_deadline
                .into_iter()
                .chain(state.rail_deadlines.values().copied())
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .min();
            state = match waits {
                Some(wait) => wake.wait_timeout(state, wait).unwrap().0,
                None => wake.wait(state).unwrap(),
            };
        }
    });
}
pub(super) fn is_command(cmd: &str) -> bool {
    cmd == "GET subsystem.clocks"
        || cmd == "ACTION subsystem.clocks.reset"
        || cmd.starts_with("ACTION subsystem.clocks.trial ")
        || cmd.starts_with("ACTION subsystem.clocks.level ")
        || cmd.starts_with("ACTION subsystem.clocks.range ")
        || cmd.starts_with("ACTION subsystem.clocks.boost ")
        || cmd.starts_with("ACTION subsystem.clocks.oem ")
}
pub(super) fn command(cmd: &str) -> Result<Value, String> {
    if cmd == "ACTION subsystem.clocks.reset" {
        restore_all()?;
    }
    if let Some(arg) = cmd.strip_prefix("ACTION subsystem.clocks.oem ") {
        let mut parts = arg.split_whitespace();
        let kind = Kind::parse(parts.next().unwrap_or_default())?;
        let rail = parts.next();
        if parts.next().is_some() {
            return Err("unexpected OEM reset argument".into());
        }
        let mut state = trials()
            .0
            .lock()
            .map_err(|_| "clock control lock poisoned")?;
        if state.journal_invalid {
            return Err("invalid recovery journal".into());
        }
        match kind {
            Kind::Storage => restore_storage(&mut state)?,
            Kind::Memory => match rail {
                Some(rail) => {
                    rail_root(rail)?;
                    restore_rail(&mut state, rail)?;
                }
                None => {
                    let rails = RAILS
                        .into_iter()
                        .filter(|rail| state.rails.contains_key(*rail))
                        .collect::<Vec<_>>();
                    for rail in rails {
                        restore_rail(&mut state, rail)?;
                    }
                }
            },
        }
        state.error = None;
        trials().1.notify_one();
    }
    if let Some((arg, range)) = cmd
        .strip_prefix("ACTION subsystem.clocks.level ")
        .map(|a| (a, false))
        .or_else(|| {
            cmd.strip_prefix("ACTION subsystem.clocks.range ")
                .map(|a| (a, true))
        })
    {
        if !service_control::enabled() {
            return Err("Peridot Essential is disabled".into());
        }
        let parts: Vec<_> = arg.split_whitespace().collect();
        if parts.len() != if range { 3 } else { 2 } {
            return Err("expected subsystem and supported frequency bounds".into());
        }
        let kind = Kind::parse(parts[0])?;
        if kind == Kind::Memory {
            return Err("memory uses DCVS boost floors; use subsystem.clocks.boost".into());
        }
        let hz = parts[1].parse::<u64>().map_err(|_| "invalid frequency")?;
        let max = if range {
            parts[2]
                .parse::<u64>()
                .map_err(|_| "invalid upper frequency")?
        } else {
            hz
        };
        let root = Kind::storage_root();
        let mut state = trials()
            .0
            .lock()
            .map_err(|_| "clock control lock poisoned")?;
        if state.journal_invalid || state.error.is_some() {
            return Err("restore previous controls first".into());
        }
        let ceiling = match state.storage {
            Some(Original::Range { max_hz, .. }) => max_hz,
            _ => number(&root, "max_freq")?,
        };
        // Reject malformed requests before altering the journal or active policy.
        validate_range(&root, hz, max, ceiling)?;
        if state.storage.is_none() {
            let min_hz = number(&root, "min_freq")?;
            let max_hz = number(&root, "max_freq")?;
            state.storage = Some(Original::Range { min_hz, max_hz });
            if let Err(e) = journal(&state) {
                state.storage = None;
                return Err(e);
            }
        }
        if let Err(e) = apply_supported_range(&root, hz, max, ceiling) {
            if let Err(restore) = restore_storage(&mut state) {
                state.error = Some(restore);
            }
            return Err(e);
        }
        state.storage_deadline = None;
        state.storage_target = Some((hz, max));
        trials().1.notify_one();
    }
    if let Some(arg) = cmd.strip_prefix("ACTION subsystem.clocks.boost ") {
        if !service_control::enabled() {
            return Err("Peridot Essential is disabled".into());
        }
        let parts = arg.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err("expected a DCVS rail and one advertised frequency".into());
        }
        let rail = parts[0];
        let hz = parts[1].parse::<u64>().map_err(|_| "invalid frequency")?;
        let root = rail_root(rail)?;
        let mut state = trials()
            .0
            .lock()
            .map_err(|_| "clock control lock poisoned")?;
        if state.journal_invalid || state.error.is_some() {
            return Err("restore previous controls first".into());
        }
        validate_boost(&root, hz)?;
        if !state.rails.contains_key(rail) {
            let original = number(&root, "boost_freq")?;
            state.rails.insert(rail.to_string(), original);
            if let Err(e) = journal(&state) {
                state.rails.remove(rail);
                return Err(e);
            }
        }
        if let Err(e) = write_boost(&root, hz) {
            if let Err(restore) = restore_rail(&mut state, rail) {
                state.error = Some(restore);
            }
            return Err(e);
        }
        state.rail_deadlines.remove(rail);
        state.rail_targets.insert(rail.to_string(), hz);
        trials().1.notify_one();
    }
    if let Some(arg) = cmd.strip_prefix("ACTION subsystem.clocks.trial ") {
        if !service_control::enabled() {
            return Err("Peridot Essential is disabled".into());
        }
        let mut parts = arg.split_whitespace();
        let kind = Kind::parse(parts.next().unwrap_or_default())?;
        let rail_arg = parts.next();
        let mut state = trials().0.lock().map_err(|_| "clock trial lock poisoned")?;
        if state.error.is_some() {
            return Err("restore the previous trial before starting another".into());
        }
        match kind {
            Kind::Storage => {
                let root = Kind::storage_root();
                let status = inspect_storage(&root)?;
                if state.storage.is_some() {
                    return Err("trial already running".into());
                }
                let min_hz = status["min_hz"].as_u64().ok_or("missing original floor")?;
                let max_hz = status["max_hz"].as_u64().ok_or("missing ceiling")?;
                state.storage = Some(Original::Range { min_hz, max_hz });
                if let Err(e) = journal(&state) {
                    state.storage = None;
                    return Err(e);
                }
                if let Err(e) = apply_max(&root) {
                    if let Err(restore) = restore_storage(&mut state) {
                        state.error = Some(restore);
                    }
                    return Err(e);
                }
                state.storage_deadline =
                    Some(Instant::now() + Duration::from_secs(TRIAL_SECONDS));
            }
            Kind::Memory => {
                let rail = rail_arg.ok_or("expected a DCVS rail for the memory trial")?;
                let root = rail_root(rail)?;
                if !rail_ready(&root) {
                    return Err("this kernel does not expose DCVS boost control for the rail".into());
                }
                if state.rails.contains_key(rail) {
                    return Err("trial already running".into());
                }
                let original = number(&root, "boost_freq")?;
                let max = number(&root, "hw_max_freq")?;
                state.rails.insert(rail.to_string(), original);
                if let Err(e) = journal(&state) {
                    state.rails.remove(rail);
                    return Err(e);
                }
                if let Err(e) = write_boost(&root, max) {
                    if let Err(restore) = restore_rail(&mut state, rail) {
                        state.error = Some(restore);
                    }
                    return Err(e);
                }
                state.rail_targets.insert(rail.to_string(), max);
                state.rail_deadlines.insert(
                    rail.to_string(),
                    Instant::now() + Duration::from_secs(TRIAL_SECONDS),
                );
            }
        }
        trials().1.notify_one();
    }
    let state = trials().0.lock().map_err(|_| "clock trial lock poisoned")?;
    let mut result =
        json!({"trial_seconds": TRIAL_SECONDS, "error": state.error, "boot_persistent": false});
    let storage_root = Kind::storage_root();
    let mut storage = inspect_storage_soft(&storage_root);
    storage["remaining_seconds"] = json!(
        state
            .storage_deadline
            .map(|d| d.saturating_duration_since(Instant::now()).as_secs() + 1)
            .unwrap_or(0)
    );
    storage["restore_pending"] = json!(state.storage.is_some());
    storage["controlled"] = json!(state.storage.is_some() && state.storage_deadline.is_none());
    if let Some((min, max)) = state.storage_target {
        storage["target_min_hz"] = json!(min);
        storage["target_max_hz"] = json!(max);
        storage["verified"] = json!(
            storage["min_hz"].as_u64() == Some(min) && storage["max_hz"].as_u64() == Some(max)
        );
    }
    if let Some(Original::Range { max_hz, .. }) = state.storage {
        storage["oem_max_hz"] = json!(max_hz);
    }
    result["storage"] = storage;

    let mut rails = serde_json::Map::new();
    let mut any_supported = false;
    for rail in RAILS {
        let root = Path::new(DCVS_BASE).join(rail);
        let mut item = rail_status(rail, &root);
        let supported = item["supported"] == true;
        any_supported |= supported;
        item["remaining_seconds"] = json!(
            state
                .rail_deadlines
                .get(rail)
                .map(|d| d.saturating_duration_since(Instant::now()).as_secs() + 1)
                .unwrap_or(0)
        );
        item["restore_pending"] = json!(state.rails.contains_key(rail));
        item["controlled"] =
            json!(state.rails.contains_key(rail) && !state.rail_deadlines.contains_key(rail));
        if let Some(&target) = state.rail_targets.get(rail) {
            item["target_hz"] = json!(target);
            item["verified"] = json!(item["boost_hz"].as_u64() == Some(target));
        }
        rails.insert(rail.to_string(), item);
    }
    result["memory"] = json!({
        "supported": any_supported,
        "base": DCVS_BASE,
        "rails": rails,
        "reason": if any_supported { Value::Null } else {
            json!("this kernel does not expose QCOM DCVS boost control")
        },
    });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage_fixture(tag: &str, name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "peridot-clock-test-{}-{tag}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        for (file, value) in [
            ("name", name),
            ("available_frequencies", "273000000 499200000"),
            ("governor", "simple_ondemand"),
            ("min_freq", "273000000"),
            ("max_freq", "499200000"),
            ("cur_freq", "273000000"),
        ] {
            fs::write(root.join(file), value).unwrap();
        }
        root
    }

    fn rail_fixture(tag: &str, boost: &str, max: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "peridot-dcvs-test-{}-{tag}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        for (file, value) in [
            ("boost_freq", boost),
            ("hw_min_freq", "547000"),
            ("hw_max_freq", max),
            ("cur_freq", "547000"),
            ("available_frequencies", "547000 1555000 2736000 4224000"),
        ] {
            fs::write(root.join(file), value).unwrap();
        }
        root
    }

    #[test]
    fn validates_and_normalizes_tables() {
        assert_eq!(
            table("499200000 273000000 499200000").unwrap(),
            vec![273000000, 499200000]
        );
        for bad in ["", "0", "-1", "90000000000", "273 nope"] {
            assert!(table(bad).is_err());
        }
        assert_eq!(rail_table("0 1").unwrap(), vec![0, 1]);
    }

    #[test]
    fn rejects_unrecognized_targets() {
        assert!(Kind::parse("../voltage").is_err());
        assert!(rail_root("../ddr").is_err());
        assert!(rail_root("gpu").is_err());
        assert!(!is_command("SET subsystem.voltage 9"));
    }

    #[test]
    fn storage_accepts_both_ufs_controller_names() {
        for name in ["1d84000.ufshc", "112b0000.ufshci"] {
            let root = storage_fixture(&name.replace('.', "-"), name);
            assert!(inspect_storage(&root).is_ok(), "{name}");
            fs::remove_dir_all(root).unwrap();
        }
        let root = storage_fixture("not-ufs", "soc:qcom,gen3_mem");
        assert!(inspect_storage(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn supported_locks_restore_both_bounds_and_preserve_other_nodes() {
        let root = storage_fixture("locks", "1d84000.ufshc");
        assert_eq!(apply_max(&root).unwrap(), 273000000);
        assert_eq!(number(&root, "min_freq").unwrap(), 499200000);
        assert_eq!(number(&root, "max_freq").unwrap(), 499200000);
        assert_eq!(
            fs::read_to_string(root.join("governor")).unwrap(),
            "simple_ondemand"
        );
        write_floor(&root, 273000000).unwrap();
        apply_level(&root, 273000000, 499200000).unwrap();
        assert_eq!(number(&root, "max_freq").unwrap(), 273000000);
        apply_level(&root, 499200000, 499200000).unwrap();
        assert_eq!(number(&root, "min_freq").unwrap(), 499200000);
        assert_eq!(number(&root, "max_freq").unwrap(), 499200000);
        assert!(apply_level(&root, 400000000, 499200000).is_err());
        assert!(apply_supported_range(&root, 499200000, 273000000, 499200000).is_err());
        apply_supported_range(&root, 273000000, 499200000, 499200000).unwrap();
        write_range(&root, 273000000, 499200000).unwrap();
        assert_eq!(number(&root, "min_freq").unwrap(), 273000000);
        assert_eq!(number(&root, "max_freq").unwrap(), 499200000);
        assert!(apply_level(&root, 499200000, 273000000).is_err());
        fs::write(root.join("max_freq"), "273000000").unwrap();
        assert!(apply_max(&root).is_err());
        assert_eq!(number(&root, "min_freq").unwrap(), 273000000);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dcvs_boost_requires_an_advertised_step() {
        let root = rail_fixture("steps", "547000", "4224000");
        validate_boost(&root, 1555000).unwrap();
        assert!(validate_boost(&root, 600000).is_err());
        assert!(validate_boost(&root, 9000000).is_err());
        write_boost(&root, 1555000).unwrap();
        assert_eq!(number(&root, "boost_freq").unwrap(), 1555000);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dcvs_boost_zero_state_is_valid_for_ddrqos() {
        let root = std::env::temp_dir().join(format!("peridot-ddrqos-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        for (file, value) in [
            ("boost_freq", "0"),
            ("hw_min_freq", "0"),
            ("hw_max_freq", "1"),
            ("cur_freq", "0"),
            ("available_frequencies", "0 1"),
        ] {
            fs::write(root.join(file), value).unwrap();
        }
        assert!(rail_ready(&root));
        validate_boost(&root, 1).unwrap();
        write_boost(&root, 1).unwrap();
        assert_eq!(number(&root, "boost_freq").unwrap(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_accepts_legacy_and_current_journals() {
        let legacy: Journal = serde_json::from_str(
            r#"{"memory":757000000,"storage":{"min_hz":273000000,"max_hz":499200000}}"#,
        )
        .unwrap();
        assert!(legacy.memory.is_some());
        assert!(matches!(
            legacy.storage,
            Some(Original::Range {
                min_hz: 273000000,
                max_hz: 499200000
            })
        ));
        let current: Journal =
            serde_json::from_str(r#"{"rails":{"ddr":547000,"l3":940800000}}"#).unwrap();
        assert_eq!(current.rails["ddr"], 547000);
        assert_eq!(current.rails["l3"], 940800000);
    }
}
