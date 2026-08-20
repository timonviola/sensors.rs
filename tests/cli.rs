//! End-to-end tests: run the real binary against a synthetic sysfs tree.
//!
//! Linux only, because the fixture emulates `/sys/class/hwmon`.
#![cfg(target_os = "linux")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sensors-rs-it-{}-{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&root);

    let cpu = root.join("hwmon0");
    fs::create_dir_all(&cpu).unwrap();
    write(&cpu, "name", "coretemp");
    write(&cpu, "temp1_label", "Package id 0");
    write(&cpu, "temp1_input", "47000");
    write(&cpu, "temp1_max", "100000");
    write(&cpu, "temp1_crit", "100000");
    write(&cpu, "temp2_label", "Core 0");
    write(&cpu, "temp2_input", "45000");
    write(&cpu, "temp2_crit", "100000");
    write(&cpu, "temp3_label", "Core 1");
    write(&cpu, "temp3_input", "49000");
    write(&cpu, "temp3_crit", "100000");

    let smc = root.join("hwmon1");
    fs::create_dir_all(&smc).unwrap();
    write(&smc, "name", "applesmc");
    write(&smc, "fan1_input", "1234");
    write(&smc, "fan1_min", "1200");
    write(&smc, "power1_label", "System total");
    write(&smc, "power1_input", "12500000");
    root
}

fn write(dir: &Path, name: &str, value: &str) {
    fs::write(dir.join(name), format!("{}\n", value)).unwrap();
}

fn sensors(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sensors"))
        .env("SENSORS_SYSFS", root)
        .args(args)
        .output()
        .expect("failed to run sensors")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn prints_lm_sensors_compatible_output() {
    let root = fixture("default");
    let out = sensors(&root, &[]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("Package id 0:  +47.0\u{b0}C  (high = +100.0\u{b0}C, crit = +100.0\u{b0}C)"),
        "{}",
        text
    );
    assert!(
        text.contains("Core 0:        +45.0\u{b0}C  (crit = +100.0\u{b0}C)"),
        "{}",
        text
    );
    assert!(
        text.contains("fan1:         1234 RPM  (min = 1200 RPM)"),
        "{}",
        text
    );
    assert!(text.contains("System total:  12.50 W"), "{}", text);
    fs::remove_dir_all(&root).unwrap();
}

/// Reproduces exactly what the tmux-cpu plugin does with our output.
#[test]
fn tmux_cpu_pipeline_yields_average_core_temp() {
    let root = fixture("tmux");
    let text = stdout(&sensors(&root, &[]));

    let mut sum = 0.0f64;
    let mut n = 0u32;
    for line in text.lines() {
        // awk '/^Core [0-9]+/ { gsub("[^0-9.]", "", $3); sum += $3; n += 1 }'
        if !line.starts_with("Core ") || !line.as_bytes()[5].is_ascii_digit() {
            continue;
        }
        let field3 = line.split_whitespace().nth(2).unwrap();
        let cleaned: String = field3
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        sum += cleaned.parse::<f64>().unwrap();
        n += 1;
    }
    assert_eq!(n, 2, "expected two Core lines in:\n{}", text);
    assert_eq!(format!("{:2.0}C", sum / n as f64), "47C");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn chip_filtering() {
    let root = fixture("filter");
    let text = stdout(&sensors(&root, &["coretemp"]));
    assert!(text.contains("Core 0"));
    assert!(!text.contains("applesmc"), "{}", text);

    let out = sensors(&root, &["doesnotexist"]);
    assert!(!out.status.success());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn fahrenheit_and_no_adapter() {
    let root = fixture("fahrenheit");
    let text = stdout(&sensors(&root, &["-f", "-A", "coretemp"]));
    assert!(text.contains("+113.0\u{b0}F"), "{}", text);
    assert!(!text.contains("Adapter:"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn raw_and_json_modes() {
    let root = fixture("modes");
    let raw = stdout(&sensors(&root, &["-u", "coretemp"]));
    assert!(raw.contains("temp2:\n  temp2_input: 45.000\n"), "{}", raw);

    let json = stdout(&sensors(&root, &["-j"]));
    assert!(json.contains("\"Core 0\":{"), "{}", json);
    assert!(json.contains("\"temp2_input\": 45.000"), "{}", json);
    assert_eq!(json.matches('{').count(), json.matches('}').count());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn help_and_version_exit_zero() {
    let root = fixture("help");
    let help = sensors(&root, &["--help"]);
    assert!(help.status.success());
    assert!(stdout(&help).starts_with("Usage: sensors"));

    let version = sensors(&root, &["-v"]);
    assert!(version.status.success());
    assert!(stdout(&version).contains("sensors version"));
    fs::remove_dir_all(&root).unwrap();
}
