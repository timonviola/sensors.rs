//! Linux backend: reads sysfs hwmon (`/sys/class/hwmon`), the same source
//! lm-sensors uses.

use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{Chip, Feature, Kind};

/// Root of the hwmon class directory. Overridable through `SENSORS_SYSFS`,
/// which is handy for testing and for inspecting a mounted sysfs snapshot.
fn hwmon_root() -> PathBuf {
    std::env::var_os("SENSORS_SYSFS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/sys/class/hwmon"))
}

pub fn collect() -> Vec<Chip> {
    let mut chips = Vec::new();
    let entries = match fs::read_dir(hwmon_root()) {
        Ok(e) => e,
        Err(_) => return chips,
    };
    let mut dirs: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        if let Some(chip) = read_hwmon(&dir) {
            if !chip.is_empty() {
                chips.push(chip);
            }
        }
    }
    chips.sort_by(|a, b| a.name.cmp(&b.name));
    chips
}

fn read_str(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_f64(path: &Path) -> Option<f64> {
    read_str(path)?.parse().ok()
}

/// `hwmonN` may keep its attributes either directly or in the legacy
/// `hwmonN/device/` subdirectory.
fn attr_dir(dir: &Path) -> PathBuf {
    if dir.join("name").exists() {
        dir.to_path_buf()
    } else {
        dir.join("device")
    }
}

fn read_hwmon(dir: &Path) -> Option<Chip> {
    let adir = attr_dir(dir);
    let name = read_str(&adir.join("name")).or_else(|| read_str(&dir.join("name")))?;
    let (bus, adapter) = bus_of(dir);
    let mut chip = Chip::new(format!("{}-{}", name, bus), adapter);

    let mut files: Vec<String> = fs::read_dir(&adir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    files.sort();

    // Group `<kind><index>_<suffix>` files into features.
    let mut seen: Vec<(Kind, usize)> = Vec::new();
    for file in &files {
        let (base, _suffix) = match file.split_once('_') {
            Some(v) => v,
            None => continue,
        };
        let (kind, index) = match parse_base(base) {
            Some(v) => v,
            None => continue,
        };
        if seen.contains(&(kind, index)) {
            continue;
        }
        seen.push((kind, index));
        if let Some(feature) = read_feature(&adir, &files, kind, index) {
            chip.features.push(feature);
        }
    }
    chip.sort();
    Some(chip)
}

fn parse_base(base: &str) -> Option<(Kind, usize)> {
    for kind in [
        Kind::Temp,
        Kind::Fan,
        Kind::Voltage,
        Kind::Power,
        Kind::Energy,
        Kind::Current,
        Kind::Humidity,
    ] {
        let p = kind.prefix();
        if let Some(rest) = base.strip_prefix(p) {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                return Some((kind, rest.parse().ok()?));
            }
        }
    }
    None
}

/// sysfs stores milli-units for most kinds and micro-units for power/energy.
fn scale(kind: Kind) -> f64 {
    match kind {
        Kind::Temp | Kind::Voltage | Kind::Current | Kind::Humidity => 1000.0,
        Kind::Power | Kind::Energy => 1_000_000.0,
        Kind::Fan | Kind::Other => 1.0,
    }
}

fn read_feature(dir: &Path, files: &[String], kind: Kind, index: usize) -> Option<Feature> {
    let base = format!("{}{}", kind.prefix(), index);
    let input = read_f64(&dir.join(format!("{}_input", base)))?;
    let label = read_str(&dir.join(format!("{}_label", base))).unwrap_or_else(|| match kind {
        Kind::Voltage => format!("in{}", index),
        _ => base.clone(),
    });
    let mut feature = Feature::new(kind, index, label);
    let div = scale(kind);
    feature.push("input", input / div);
    for suffix in ["min", "max", "crit", "emergency", "alarm", "cap", "average"] {
        let fname = format!("{}_{}", base, suffix);
        if !files.iter().any(|f| f == &fname) {
            continue;
        }
        if let Some(v) = read_f64(&dir.join(&fname)) {
            let d = if suffix == "alarm" { 1.0 } else { div };
            feature.push(suffix, v / d);
        }
    }
    Some(feature)
}

/// Derives the `<bus>-<address>` chip name suffix and the adapter string.
fn bus_of(dir: &Path) -> (String, String) {
    let device = dir.join("device");
    let real = fs::canonicalize(&device).unwrap_or_else(|_| dir.to_path_buf());
    let subsystem = fs::canonicalize(device.join("subsystem"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_default();
    let base = real
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    match subsystem.as_str() {
        "i2c" => {
            // Format: <bus>-<addr>, e.g. "0-004c".
            if let Some((bus, addr)) = base.split_once('-') {
                let n: u32 = bus.parse().unwrap_or(0);
                return (
                    format!("i2c-{}-{}", n, addr.trim_start_matches('0')),
                    i2c_adapter(&real),
                );
            }
            ("i2c-0-0000".into(), "I2C adapter".into())
        }
        "pci" => (format!("pci-{}", pci_id(&base)), "PCI adapter".into()),
        "platform" | "of_platform" => (isa_addr(&real), "ISA adapter".into()),
        "acpi" => ("acpi-0".into(), "ACPI interface".into()),
        "hid" => ("hid-3-1".into(), "HID adapter".into()),
        "spi" => ("spi-0".into(), "SPI adapter".into()),
        "virtual" | "" => ("virtual-0".into(), "Virtual device".into()),
        other => (format!("{}-0", other), format!("{} adapter", other)),
    }
}

fn i2c_adapter(dev: &Path) -> String {
    dev.parent()
        .map(|p| p.join("name"))
        .and_then(|p| read_str(&p))
        .map(|n| format!("{}", n))
        .unwrap_or_else(|| "I2C adapter".to_string())
}

fn pci_id(base: &str) -> String {
    // "0000:00:18.3" -> "00d8" style short id; keep the last two components.
    let parts: Vec<&str> = base.split(':').collect();
    if parts.len() == 3 {
        let devfn = parts[2].replace('.', "");
        format!("{}{}", parts[1], devfn)
    } else {
        base.to_string()
    }
}

fn isa_addr(dev: &Path) -> String {
    let base = dev
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    match base.rsplit_once('.') {
        Some((_, n)) if n.chars().all(|c| c.is_ascii_digit()) => {
            format!("isa-{:04x}", n.parse::<u32>().unwrap_or(0))
        }
        _ => "isa-0000".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_parsing() {
        assert_eq!(parse_base("temp1"), Some((Kind::Temp, 1)));
        assert_eq!(parse_base("in12"), Some((Kind::Voltage, 12)));
        assert_eq!(parse_base("fan2"), Some((Kind::Fan, 2)));
        assert_eq!(parse_base("power1"), Some((Kind::Power, 1)));
        assert_eq!(parse_base("name"), None);
        assert_eq!(parse_base("temp"), None);
    }

    #[test]
    fn scaling_rules() {
        assert_eq!(scale(Kind::Temp), 1000.0);
        assert_eq!(scale(Kind::Power), 1_000_000.0);
        assert_eq!(scale(Kind::Fan), 1.0);
    }

    #[test]
    fn pci_ids() {
        assert_eq!(pci_id("0000:00:18.3"), "00183");
    }

    #[test]
    fn reads_a_synthetic_hwmon_tree() {
        let dir = std::env::temp_dir().join(format!("sensors-rs-hwmon-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("name"), "coretemp\n").unwrap();
        fs::write(dir.join("temp1_label"), "Package id 0\n").unwrap();
        fs::write(dir.join("temp1_input"), "47000\n").unwrap();
        fs::write(dir.join("temp1_crit"), "100000\n").unwrap();
        fs::write(dir.join("fan1_input"), "1234\n").unwrap();

        let chip = read_hwmon(&dir).unwrap();
        assert!(chip.name.starts_with("coretemp-"));
        let temp = chip.features.iter().find(|f| f.name == "temp1").unwrap();
        assert_eq!(temp.label, "Package id 0");
        assert_eq!(temp.input(), Some(47.0));
        assert_eq!(temp.get("crit"), Some(100.0));
        let fan = chip.features.iter().find(|f| f.name == "fan1").unwrap();
        assert_eq!(fan.input(), Some(1234.0));
        fs::remove_dir_all(&dir).unwrap();
    }
}
