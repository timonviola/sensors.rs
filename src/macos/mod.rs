//! macOS backend: assembles chips from the SMC and the HID temperature
//! sensors.
//!
//! Three chips are produced:
//!   * `cpu_thermal-*`  - synthesized CPU chip with `Package id 0` / `Core N`
//!     labels, so that scrapers written for lm-sensors
//!     (e.g. the tmux-cpu plugin) keep working.
//!   * `applesmc-isa-0300` - fans, power rails and SMC temperature keys, named
//!     after the Linux `applesmc` driver.
//!   * `soc_thermal-hid-0` - every raw Apple HID temperature sensor.

mod ffi;
mod hid;
mod smc;

use crate::apple_map::{
    core_labels, package_temp, plausible_temp, smc_core_index, smc_fan_label, smc_power_label,
    smc_temp_label,
};
use crate::model::{Chip, Feature, Kind};

pub fn collect() -> Vec<Chip> {
    let hid_sensors: Vec<(String, f64)> = hid::read_temperatures()
        .into_iter()
        .map(|s| (s.name, s.temperature))
        .collect();
    let smc = smc::Smc::open();

    let mut chips = Vec::new();
    if let Some(chip) = cpu_chip(&hid_sensors, smc.as_ref()) {
        chips.push(chip);
    }
    if let Some(smc) = smc.as_ref() {
        if let Some(chip) = smc_chip(smc) {
            chips.push(chip);
        }
    }
    if let Some(chip) = hid_chip(&hid_sensors) {
        chips.push(chip);
    }
    chips
}

/// Builds the `Core N` chip, preferring HID sensors (Apple Silicon) and
/// falling back to the Intel `TC?C` SMC keys.
fn cpu_chip(hid_sensors: &[(String, f64)], smc: Option<&smc::Smc>) -> Option<Chip> {
    let mut cores = core_labels(hid_sensors);
    let mut package = package_temp(hid_sensors);
    let mut adapter = "HID Sensors";

    if cores.is_empty() {
        let smc = smc?;
        adapter = "SMC";
        for i in 0..10u32 {
            let key = format!("TC{}C", i);
            if let Some(v) = smc.read(&key).filter(|v| plausible_temp(*v)) {
                cores.push((format!("Core {}", i), v));
            }
        }
        package = ["TCXC", "TCXc", "TC0D", "TC0P"]
            .iter()
            .find_map(|k| smc.read(k).filter(|v| plausible_temp(*v)));
    }
    if cores.is_empty() && package.is_none() {
        return None;
    }

    let bus = if adapter == "SMC" {
        "isa-0000"
    } else {
        "hid-0"
    };
    let mut chip = Chip::new(format!("cpu_thermal-{}", bus), adapter);
    let mut index = 1;
    if let Some(p) = package {
        let mut f = Feature::new(Kind::Temp, index, "Package id 0");
        f.push("input", p);
        chip.features.push(f);
        index += 1;
    }
    for (label, value) in cores {
        let mut f = Feature::new(Kind::Temp, index, label);
        f.push("input", value);
        chip.features.push(f);
        index += 1;
    }
    Some(chip)
}

fn smc_chip(smc: &smc::Smc) -> Option<Chip> {
    let keys = smc.keys();
    if keys.is_empty() {
        return None;
    }
    let mut chip = Chip::new("applesmc-isa-0300", "SMC");

    // Fans: F<n>Ac actual / F<n>Mn min / F<n>Mx max / F<n>Tg target.
    let fan_count = smc.read("FNum").unwrap_or(0.0) as u32;
    for i in 0..fan_count.min(10) {
        let actual = match smc.read(&format!("F{}Ac", i)) {
            Some(v) if v.is_finite() => v,
            _ => continue,
        };
        let mut f = Feature::new(Kind::Fan, i as usize + 1, smc_fan_label(i));
        f.push("input", actual);
        if let Some(v) = smc.read(&format!("F{}Mn", i)) {
            f.push("min", v);
        }
        if let Some(v) = smc.read(&format!("F{}Mx", i)) {
            f.push("max", v);
        }
        chip.features.push(f);
    }

    let mut temp_i = 1;
    let mut power_i = 1;
    let mut volt_i = 0;
    let mut curr_i = 1;
    for key in &keys {
        let first = key.as_bytes()[0];
        match first {
            b'T' => {
                let label = match smc_temp_label(key) {
                    Some(l) => l.to_string(),
                    None => match smc_core_index(key) {
                        Some(n) => format!("CPU core {}", n),
                        // Undocumented keys are skipped rather than shown raw:
                        // most of them are not temperatures at all.
                        None => continue,
                    },
                };
                if let Some(v) = smc.read(key).filter(|v| plausible_temp(*v)) {
                    let mut f = Feature::new(Kind::Temp, temp_i, format!("{} ({})", label, key));
                    f.push("input", v);
                    chip.features.push(f);
                    temp_i += 1;
                }
            }
            b'P' | b'V' | b'I' => {
                let label = match smc_power_label(key) {
                    Some(l) => l,
                    None => continue,
                };
                let value = match smc.read(key) {
                    Some(v) if v.is_finite() && v.abs() < 1000.0 => v,
                    _ => continue,
                };
                let (kind, index) = match first {
                    b'P' => (Kind::Power, &mut power_i),
                    b'V' => (Kind::Voltage, &mut volt_i),
                    _ => (Kind::Current, &mut curr_i),
                };
                let mut f = Feature::new(kind, *index, format!("{} ({})", label, key));
                f.push("input", value);
                chip.features.push(f);
                *index += 1;
            }
            _ => {}
        }
    }

    if chip.is_empty() {
        None
    } else {
        chip.sort();
        Some(chip)
    }
}

fn hid_chip(sensors: &[(String, f64)]) -> Option<Chip> {
    if sensors.is_empty() {
        return None;
    }
    let mut chip = Chip::new("soc_thermal-hid-0", "HID Sensors");
    for (i, (name, value)) in sensors.iter().enumerate() {
        let mut f = Feature::new(Kind::Temp, i + 1, name.clone());
        f.push("input", *value);
        chip.features.push(f);
    }
    Some(chip)
}
