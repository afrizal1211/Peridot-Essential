#![cfg(not(target_os = "android"))]
use peridot_essential_backend::handle_command;
use std::{fs, process::Command};
#[test]
fn sconfig_keeps_original_across_modes_disable_and_process_restart() {
    if let Ok(expected) = std::env::var("ONYX_TEST_SCONFIG_RESUME") {
        assert_eq!(handle_command("SET service.enabled 1"), "OK applied");
        let node = std::env::var("ONYX_TEST_SCONFIG_PATH").unwrap();
        assert_eq!(fs::read_to_string(node).unwrap(), expected);
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "peridot-sconfig-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let node = dir.join("sconfig");
    fs::write(&node, "0").unwrap();
    fs::write(
        dir.join("state.conf"),
        "perf=-1\nzram_size_mb=-1\nzram_algorithm=\nzram_swappiness=-1\ncpu_manual=-1\n",
    )
    .unwrap();
    unsafe {
        std::env::set_var("ONYX_STATE_DIR", &dir);
        std::env::set_var("ONYX_TEST_SCONFIG_PATH", &node);
    }
    for mode in [6, 18, 56, 68] {
        assert_eq!(
            handle_command(&format!("SET cpu.sconfig {mode}")),
            "OK applied"
        );
        assert_eq!(fs::read_to_string(&node).unwrap(), mode.to_string());
        assert!(
            handle_command("GET snapshot")
                .split(';')
                .any(|v| v == "sconfig_original=0")
        );
    }
    assert!(handle_command("SET cpu.sconfig 123").starts_with("ERR "));
    assert_eq!(handle_command("SET cpu.sconfig -1"), "OK applied");
    assert_eq!(fs::read_to_string(&node).unwrap(), "0");
    assert_eq!(handle_command("SET cpu.sconfig 6"), "OK applied");
    assert_eq!(handle_command("SET service.enabled 0"), "OK applied");
    assert_eq!(fs::read_to_string(&node).unwrap(), "0");
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "sconfig_keeps_original_across_modes_disable_and_process_restart",
        ])
        .env("ONYX_TEST_SCONFIG_RESUME", "6")
        .status()
        .unwrap();
    assert!(status.success());
    // The parent intentionally retains its disabled intent; re-enable to reload
    // the selected policy in this process before testing explicit OEM release.
    assert_eq!(handle_command("SET service.enabled 1"), "OK applied");
    fs::remove_file(&node).unwrap();
    assert!(handle_command("SET cpu.sconfig 18").starts_with("ERR "));
    fs::write(&node, "6").unwrap();
    assert_eq!(handle_command("SET cpu.sconfig -1"), "OK applied");
    assert_eq!(fs::read_to_string(&node).unwrap(), "0");
    // An OEM value of -1 must be restored literally, never guessed as zero.
    fs::write(&node, "-1").unwrap();
    assert_eq!(handle_command("SET cpu.sconfig 6"), "OK applied");
    assert_eq!(handle_command("SET cpu.sconfig -1"), "OK applied");
    assert_eq!(fs::read_to_string(&node).unwrap(), "-1");
    assert_eq!(handle_command("SET cpu.sconfig 6"), "OK applied");
    assert_eq!(handle_command("ACTION service.reset"), "OK applied");
    assert_eq!(fs::read_to_string(&node).unwrap(), "-1");
    assert!(
        fs::read_to_string(dir.join("state.conf"))
            .unwrap()
            .lines()
            .any(|line| line == "sconfig_override=-1")
    );
    fs::remove_dir_all(dir).unwrap();
}
