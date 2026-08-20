//! Runs the WezTerm Lua plugin's test suite, driving the real `sensors`
//! binary through it.
//!
//! Skipped (as a pass) when no Lua interpreter is installed, so `cargo test`
//! keeps working on machines without one.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn lua_interpreter() -> Option<&'static str> {
    ["lua5.4", "lua5.3", "lua", "luajit"]
        .into_iter()
        .find(|bin| Command::new(bin).arg("-v").output().is_ok())
}

fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("sensors-rs-lua-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let hwmon = root.join("hwmon0");
    fs::create_dir_all(&hwmon).unwrap();
    fs::write(hwmon.join("name"), "coretemp\n").unwrap();
    for i in 0..4 {
        let n = i + 1;
        fs::write(
            hwmon.join(format!("temp{}_label", n)),
            format!("Core {}\n", i),
        )
        .unwrap();
        fs::write(
            hwmon.join(format!("temp{}_input", n)),
            format!("{}\n", 44000 + i * 2000),
        )
        .unwrap();
    }
    root
}

#[test]
fn wezterm_plugin_test_suite_passes() {
    let lua = match lua_interpreter() {
        Some(l) => l,
        None => {
            eprintln!("skipping: no Lua interpreter found");
            return;
        }
    };

    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lua/test_cpu_temp.lua");
    let root = fixture();

    let out = Command::new(lua)
        .arg(&script)
        .arg(env!("CARGO_BIN_EXE_sensors"))
        .env("SENSORS_SYSFS", &root)
        .output()
        .expect("failed to run lua");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "lua tests failed:\n{}\n{}",
        stdout,
        stderr
    );
    assert!(stdout.contains("0 failures"), "{}", stdout);
    // The sysfs fixture only applies on Linux; averaging 44/46/48/50 C must
    // render as 47 C through the plugin. On macOS the binary reads the real
    // hardware, so the plugin's own "real binary produces a reading" check
    // (covered by "0 failures" above) is what validates the rendering.
    if cfg!(target_os = "linux") {
        assert!(stdout.contains("CPU 47\u{b0}C"), "{}", stdout);
    }

    let _ = fs::remove_dir_all(&root);
}
