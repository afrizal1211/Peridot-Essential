//! POCO F7 KGSL backend. Frequencies are validated against the live OPP table.
use super::{read_trimmed, service_control};
const GPU: &str = "/sys/class/kgsl/kgsl-3d0";
pub(super) fn root_read(node: &str) -> Option<String> {
    read_trimmed(format!("{GPU}/{node}")).ok()
}
pub(super) fn read(node: &str) -> Option<String> {
    root_read(&format!("devfreq/{node}"))
}
pub(super) fn write(node: &str, value: &str) -> Result<(), String> {
    let path = format!("{GPU}/devfreq/{node}");
    service_control::write(&path, value).map_err(|e| e.to_string())?;
    if read(node).as_deref() != Some(value) {
        return Err(format!(
            "KGSL {node} verify requested={value} actual={:?}",
            read(node)
        ));
    }
    Ok(())
}
pub(super) fn mhz(node: &str) -> Option<i32> {
    Some((read(node)?.parse::<i64>().ok()? / 1_000_000) as i32)
}
fn parse_table(raw: &str) -> Vec<i32> {
    let mut v: Vec<i32> = raw
        .split_whitespace()
        .filter_map(|v| v.parse::<i64>().ok())
        .filter(|v| *v > 0 && *v % 1_000_000 == 0)
        .map(|v| (v / 1_000_000) as i32)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}
fn table() -> Result<Vec<i32>, String> {
    let raw = read("available_frequencies")
        .or_else(|| root_read("gpu_available_frequencies"))
        .ok_or("KGSL OPP table unavailable")?;
    let v = parse_table(&raw);
    if v.is_empty() {
        Err("KGSL OPP table empty".into())
    } else {
        Ok(v)
    }
}
fn remember_defaults() -> Result<(), String> {
    if service_control::original("peridot.gpu.defaults").is_none() {
        let min = mhz("min_freq").ok_or("KGSL minimum unavailable")?;
        let max = mhz("max_freq").ok_or("KGSL maximum unavailable")?;
        service_control::remember("peridot.gpu.defaults", serde_json::json!([min, max]))?;
    }
    Ok(())
}
fn choose_range(profile: i32, opps: &[i32], stock: (i32, i32)) -> Result<(i32, i32), String> {
    let min = *opps.first().ok_or("GPU table empty")?;
    let max = *opps.last().ok_or("GPU table empty")?;
    match profile {
        0 => Ok(stock),
        1 => Ok((min, max)),
        2 => Ok((
            min,
            *opps
                .iter()
                .filter(|v| **v <= 510)
                .next_back()
                .unwrap_or(&min),
        )),
        3 => Ok((max, max)),
        _ => Err("invalid GPU profile".into()),
    }
}
pub(super) fn profile_range(profile: i32) -> Result<(i32, i32), String> {
    let opps = table()?;
    remember_defaults()?;
    let saved = service_control::original("peridot.gpu.defaults").ok_or("GPU defaults unavailable")?;
    let stock = (
        saved[0].as_i64().ok_or("invalid GPU default")? as i32,
        saved[1].as_i64().ok_or("invalid GPU default")? as i32,
    );
    choose_range(profile, &opps, stock)
}
pub(super) fn set_range(min: i32, max: i32) -> Result<(), String> {
    let opps = table()?;
    if min > max || !opps.contains(&min) || !opps.contains(&max) {
        return Err(format!(
            "KGSL range {min}-{max} MHz is not in the live OPP table"
        ));
    }
    remember_defaults()?;
    let old_min = mhz("min_freq").ok_or("KGSL minimum unavailable")?;
    let old_max = mhz("max_freq").ok_or("KGSL maximum unavailable")?;
    let raw_read = || -> Result<(i64, i64), String> {
        let get = |node: &str| -> Result<i64, String> {
            read(node)
                .ok_or_else(|| format!("KGSL {node} unavailable"))?
                .parse()
                .map_err(|e| format!("KGSL {node} invalid: {e}"))
        };
        Ok((get("min_freq")?, get("max_freq")?))
    };
    let status = super::limits::submit(
        i64::from(min) * 1_000_000,
        i64::from(max) * 1_000_000,
        (
            i64::from(old_min) * 1_000_000,
            i64::from(old_max) * 1_000_000,
        ),
        |bound, value| {
            let node = match bound {
                super::limits::Bound::Min => "min_freq",
                super::limits::Bound::Max => "max_freq",
            };
            service_control::write(format!("{GPU}/devfreq/{node}"), value.to_string())
                .map_err(|e| format!("KGSL {node} requested {value} Hz: {e}"))
        },
        raw_read,
    )?;
    eprintln!(
        "PERIDOT_GPU_REQUEST requested_mhz={min}-{max} effective_hz={}-{} exact={}",
        status.min, status.max, status.exact
    );
    Ok(())
}

pub(super) fn apply_profile(profile: i32) -> Result<(), String> {
    let (min, max) = profile_range(profile)?;
    if !read("available_governors")
        .unwrap_or_default()
        .split_whitespace()
        .any(|v| v == "msm-adreno-tz")
    {
        return Err("msm-adreno-tz unavailable".into());
    }
    write("governor", "msm-adreno-tz")?;
    set_range(min, max)
}
pub(super) fn profile_verified(profile: i32) -> bool {
    profile_range(profile).is_ok_and(|(min, max)| {
        mhz("min_freq") == Some(min)
            && mhz("max_freq") == Some(max)
            && read("governor").as_deref() == Some("msm-adreno-tz")
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn peridot_profiles_use_real_opps() {
        let opps = parse_table("1150000000 873000000 510000000 362000000 264000000");
        assert_eq!(choose_range(0, &opps, (264, 873)).unwrap(), (264, 873));
        assert_eq!(choose_range(1, &opps, (264, 873)).unwrap(), (264, 1150));
        assert_eq!(choose_range(2, &opps, (264, 873)).unwrap(), (264, 510));
        assert_eq!(choose_range(3, &opps, (264, 873)).unwrap(), (1150, 1150));
        assert!(choose_range(4, &opps, (264, 873)).is_err());
    }
}
