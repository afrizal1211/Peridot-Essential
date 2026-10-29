#![cfg(not(target_os = "android"))]

use peridot_essential_backend::handle_command;
use std::fs;
use std::process::Command;

#[test]
fn service_lifecycle_survives_process_restart() {
    if let Ok(expected) = std::env::var("ONYX_TEST_EXPECT_ENABLED") {
        let snapshot = handle_command("GET snapshot");
        assert!(
            snapshot.contains(&format!("service_enabled={expected}")),
            "{snapshot}"
        );
        if expected == "0" {
            assert!(handle_command("SET cpu.gov 0 performance").contains("disabled"));
            assert!(handle_command("SET system.colors.wallpaper").contains("disabled"));
            assert!(handle_command("ACTION gpu.uncap_full_speed").contains("disabled"));
        }
        return;
    }
    let dir = std::env::temp_dir().join(
        format!("peridot-service-test-{}", std::process::id())
            + &format!(
                "-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ),
    );
    fs::create_dir(&dir).unwrap();
    // This test executable has one test, with no concurrent environment access.
    unsafe {
        std::env::set_var("ONYX_STATE_DIR", &dir);
    }
    assert_eq!(handle_command("SET service.enabled 0"), "OK applied");
    let disabled = fs::read_to_string(dir.join("service.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&disabled).unwrap()["enabled"],
        false
    );
    let run_child = |enabled: &str| {
        let status = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("service_lifecycle_survives_process_restart")
            .env("ONYX_TEST_EXPECT_ENABLED", enabled)
            .status()
            .unwrap();
        assert!(status.success());
    };
    run_child("0");
    assert_eq!(handle_command("SET service.enabled 1"), "OK applied");
    // Touch OEM selection now requires verified game-mode access.
    assert!(handle_command("SET touch 0").starts_with("ERR "));
    for command in ["SET display.color -1", "SET display.temp -1"] {
        assert_eq!(handle_command(command), "OK applied", "{command}");
    }
    let oem = handle_command("GET snapshot");
    for field in ["touch=-1", "display_color=-1", "display_temp=-1"] {
        assert!(oem.split(';').any(|value| value == field), "{oem}");
    }
    // Missing KGSL must not publish a successfully applied profile.
    let before = handle_command("GET snapshot")
        .split(';')
        .find(|v| v.starts_with("perf="))
        .unwrap()
        .to_string();
    assert!(handle_command("SET perf 2").starts_with("ERR "));
    assert!(
        handle_command("GET snapshot")
            .split(';')
            .any(|v| v == before)
    );
    assert_eq!(handle_command("ACTION service.reset"), "OK applied");
    let snapshot = handle_command("GET snapshot");
    for field in ["perf=-1", "touch=-1", "display_color=-1", "display_temp=-1"] {
        assert!(
            snapshot.split(';').any(|value| value == field),
            "stale {field}: {snapshot}"
        );
    }
    let reset = fs::read_to_string(dir.join("state.conf")).unwrap();
    for setting in [
        "charging=-1",
        "touch=-1",
        "cpu_manual=-1",
        "perf=-1",
        "zram_size_mb=-1",
        "zram_algorithm=",
        "zram_swappiness=-1",
        "display_color=-1",
        "display_temp=-1",
        "display_width=-1",
        "display_height=-1",
        "display_density=-1",
        "sunlight=-1",
        "silky=-1",
        "video=-1",
        "dolby=-1",
        "gpu_min_freq_mhz=-1",
        "gpu_max_freq_mhz=-1",
        "gpu_governor=",
        "gpu_power_policy=",
        "gpu_ged_boost=-1",
        "cpu_min_freq0=-1",
        "cpu_max_freq0=-1",
        "cpu0=",
        "cpu2=",
        "cpu5=",
        "cpu7=",
        "io=",
        "bypass_charging=0",
    ] {
        assert!(
            reset.lines().any(|line| line == setting),
            "missing {setting}"
        );
    }
    let control: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("service.json")).unwrap()).unwrap();
    assert_eq!(control["enabled"], true);
    assert_eq!(control["configured"], false);
    assert_eq!(control["release_pending"], false);
    run_child("1");
    // Only remove the exact temporary test directory created above.
    fs::remove_file(dir.join("state.conf")).unwrap();
    fs::remove_file(dir.join("service.json")).unwrap();
    fs::remove_file(dir.join("app-profiles.json")).unwrap();
    fs::remove_dir(dir).unwrap();
}
