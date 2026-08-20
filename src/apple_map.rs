//! Pure mapping helpers that turn raw Apple sensor identifiers (HID product
//! names and 4-character SMC keys) into libsensors-style labels.
//!
//! Kept free of any platform API so the logic is unit-testable everywhere.

/// Classification of an Apple HID temperature sensor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HidClass {
    /// Performance CPU core cluster sensor.
    PerfCore,
    /// Efficiency CPU core cluster sensor.
    EffCore,
    /// Whole-SoC / package sensor.
    Soc,
    Gpu,
    Other,
}

pub fn classify_hid(name: &str) -> HidClass {
    let n = name.to_ascii_lowercase();
    if n.starts_with("pacc") {
        HidClass::PerfCore
    } else if n.starts_with("eacc") {
        HidClass::EffCore
    } else if n.contains("cpu") && n.contains("core") {
        HidClass::PerfCore
    } else if n.starts_with("soc") || n.contains("pmgr soc") || n.contains("soc mtr") {
        HidClass::Soc
    } else if n.starts_with("gpu") {
        HidClass::Gpu
    } else {
        HidClass::Other
    }
}

/// Trailing integer of a sensor name, used to order `... Sensor0/1/2`.
pub fn trailing_index(name: &str) -> u32 {
    let digits: String = name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().unwrap_or(0)
}

/// Builds the ordered `Core N` list from HID sensors: performance cores first,
/// then efficiency cores, each ordered by their trailing index.
///
/// Input is `(name, temperature)`; output is `(label, temperature)`.
pub fn core_labels(sensors: &[(String, f64)]) -> Vec<(String, f64)> {
    let mut perf: Vec<&(String, f64)> = Vec::new();
    let mut eff: Vec<&(String, f64)> = Vec::new();
    for s in sensors {
        match classify_hid(&s.0) {
            HidClass::PerfCore => perf.push(s),
            HidClass::EffCore => eff.push(s),
            _ => {}
        }
    }
    perf.sort_by_key(|s| trailing_index(&s.0));
    eff.sort_by_key(|s| trailing_index(&s.0));
    perf.into_iter()
        .chain(eff)
        .enumerate()
        .map(|(i, s)| (format!("Core {}", i), s.1))
        .collect()
}

/// Picks the package temperature: an explicit SoC sensor if present, otherwise
/// the hottest core.
pub fn package_temp(sensors: &[(String, f64)]) -> Option<f64> {
    if let Some(s) = sensors.iter().find(|s| classify_hid(&s.0) == HidClass::Soc) {
        return Some(s.1);
    }
    core_labels(sensors)
        .iter()
        .map(|c| c.1)
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a: f64| a.max(v)))
        })
}

/// Friendly label for a known SMC temperature key.
pub fn smc_temp_label(key: &str) -> Option<&'static str> {
    Some(match key {
        "TCXC" | "TCXc" => "CPU package",
        "TC0P" => "CPU proximity",
        "TC0D" | "TC0E" | "TC0F" => "CPU die",
        "TC0H" => "CPU heatsink",
        "TCAD" => "CPU package alt",
        "TG0P" => "GPU proximity",
        "TG0D" => "GPU die",
        "TG0H" => "GPU heatsink",
        "TA0P" | "TA1P" => "Ambient",
        "Ts0P" | "Ts0S" | "Ts1P" | "Ts1S" => "Palm rest / skin",
        "TM0P" | "TM0S" => "Memory proximity",
        "TB0T" | "TB1T" | "TB2T" => "Battery",
        "TW0P" => "Wireless module",
        "TH0P" | "TH0a" => "Drive / heatsink",
        "TN0P" | "TN0D" => "Northbridge",
        "TPCD" => "PCH die",
        "TL0P" => "Display",
        "TI0P" => "Thunderbolt",
        "TN0C" => "MCP",
        "Te0T" | "Te0S" => "SoC",
        _ => return None,
    })
}

/// Recognises Intel per-core SMC keys `TC0C`..`TC9C` and returns the core index.
pub fn smc_core_index(key: &str) -> Option<u32> {
    let b = key.as_bytes();
    if b.len() == 4 && b[0] == b'T' && b[1] == b'C' && b[3] == b'C' && b[2].is_ascii_digit() {
        Some((b[2] - b'0') as u32)
    } else {
        None
    }
}

/// Friendly label for a known SMC power/voltage/current key.
pub fn smc_power_label(key: &str) -> Option<&'static str> {
    Some(match key {
        "PSTR" => "System total",
        "PDTR" => "DC in total",
        "PCPC" => "CPU cores",
        "PCPG" => "GPU",
        "PCPT" => "CPU total",
        "PCTR" => "CPU rail",
        "PC0C" => "CPU core rail",
        "PGTR" => "GPU rail",
        "PZ0S" | "PZ1S" => "System rail",
        "PPBR" => "Battery",
        "VP0R" => "DC in",
        "VD0R" => "DC in rail",
        "VC0C" => "CPU core",
        "VG0C" => "GPU core",
        "VM0R" => "Memory",
        "VBAT" | "B0AV" => "Battery",
        "ID0R" => "DC in",
        "IB0R" => "Battery",
        "IC0C" => "CPU core",
        "IG0C" => "GPU core",
        _ => return None,
    })
}

/// Fan label, `F0Ac` -> `Fan 1`.
pub fn smc_fan_label(index: u32) -> String {
    format!("Fan {}", index + 1)
}

/// Sanity filter for temperature readings coming out of the SMC.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn plausible_temp(v: f64) -> bool {
    v.is_finite() && v > 1.0 && v < 150.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, v: f64) -> (String, f64) {
        (name.to_string(), v)
    }

    #[test]
    fn classifies_apple_silicon_sensor_names() {
        assert_eq!(classify_hid("pACC MTR Temp Sensor0"), HidClass::PerfCore);
        assert_eq!(classify_hid("eACC MTR Temp Sensor1"), HidClass::EffCore);
        assert_eq!(classify_hid("SOC MTR Temp Sensor0"), HidClass::Soc);
        assert_eq!(classify_hid("GPU MTR Temp Sensor2"), HidClass::Gpu);
        assert_eq!(classify_hid("NAND CH0 temp"), HidClass::Other);
        assert_eq!(classify_hid("gas gauge battery"), HidClass::Other);
    }

    #[test]
    fn cores_are_numbered_perf_first() {
        let sensors = vec![
            s("eACC MTR Temp Sensor1", 40.0),
            s("pACC MTR Temp Sensor1", 52.0),
            s("eACC MTR Temp Sensor0", 39.0),
            s("pACC MTR Temp Sensor0", 51.0),
            s("SOC MTR Temp Sensor0", 47.0),
        ];
        let cores = core_labels(&sensors);
        assert_eq!(
            cores,
            vec![
                ("Core 0".to_string(), 51.0),
                ("Core 1".to_string(), 52.0),
                ("Core 2".to_string(), 39.0),
                ("Core 3".to_string(), 40.0),
            ]
        );
    }

    #[test]
    fn package_prefers_soc_sensor() {
        let sensors = vec![
            s("pACC MTR Temp Sensor0", 51.0),
            s("SOC MTR Temp Sensor0", 47.0),
        ];
        assert_eq!(package_temp(&sensors), Some(47.0));
    }

    #[test]
    fn package_falls_back_to_hottest_core() {
        let sensors = vec![
            s("pACC MTR Temp Sensor0", 51.0),
            s("eACC MTR Temp Sensor0", 61.0),
        ];
        assert_eq!(package_temp(&sensors), Some(61.0));
        assert_eq!(package_temp(&[]), None);
    }

    #[test]
    fn trailing_indices() {
        assert_eq!(trailing_index("pACC MTR Temp Sensor12"), 12);
        assert_eq!(trailing_index("no digits"), 0);
    }

    #[test]
    fn smc_key_labels() {
        assert_eq!(smc_temp_label("TC0P"), Some("CPU proximity"));
        assert_eq!(smc_temp_label("XXXX"), None);
        assert_eq!(smc_core_index("TC3C"), Some(3));
        assert_eq!(smc_core_index("TC0P"), None);
        assert_eq!(smc_power_label("PSTR"), Some("System total"));
        assert_eq!(smc_fan_label(0), "Fan 1");
    }
}
