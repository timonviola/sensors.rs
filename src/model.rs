//! Platform independent sensor data model.
//!
//! The shape mirrors libsensors: a chip has features (`temp1`, `fan1`, ...) and
//! every feature has sub-features (`temp1_input`, `temp1_crit`, ...).

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub enum Kind {
    // `Other` is a fallback for sub-features that carry no unit.
    Voltage,
    Fan,
    Temp,
    Power,
    Energy,
    Current,
    Humidity,
    #[allow(dead_code)]
    Other,
}

impl Kind {
    /// Sub-feature name prefix used by libsensors (`temp1_input` -> `temp`).
    pub fn prefix(self) -> &'static str {
        match self {
            Kind::Voltage => "in",
            Kind::Fan => "fan",
            Kind::Temp => "temp",
            Kind::Power => "power",
            Kind::Energy => "energy",
            Kind::Current => "curr",
            Kind::Humidity => "humidity",
            Kind::Other => "value",
        }
    }
}

/// A single measured quantity, e.g. `temp1_input` or `temp1_crit`.
#[derive(Clone, Debug)]
pub struct Subfeature {
    pub name: String,
    pub value: f64,
}

impl Subfeature {
    pub fn new(name: impl Into<String>, value: f64) -> Self {
        Subfeature {
            name: name.into(),
            value,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Feature {
    /// Internal name, e.g. `temp1`.
    pub name: String,
    /// Human readable label, e.g. `Core 0`.
    pub label: String,
    pub kind: Kind,
    pub subs: Vec<Subfeature>,
}

impl Feature {
    pub fn new(kind: Kind, index: usize, label: impl Into<String>) -> Self {
        let name = format!("{}{}", kind.prefix(), index);
        Feature {
            name,
            label: label.into(),
            kind,
            subs: Vec::new(),
        }
    }

    /// Adds `<name><index>_<suffix>` with the given value.
    pub fn push(&mut self, suffix: &str, value: f64) -> &mut Self {
        let name = format!("{}_{}", self.name, suffix);
        self.subs.push(Subfeature::new(name, value));
        self
    }

    pub fn get(&self, suffix: &str) -> Option<f64> {
        let want = format!("{}_{}", self.name, suffix);
        self.subs.iter().find(|s| s.name == want).map(|s| s.value)
    }

    pub fn input(&self) -> Option<f64> {
        self.get("input")
    }
}

#[derive(Clone, Debug)]
pub struct Chip {
    /// Full chip name as printed by sensors, e.g. `coretemp-isa-0000`.
    pub name: String,
    pub adapter: String,
    pub features: Vec<Feature>,
}

impl Chip {
    pub fn new(name: impl Into<String>, adapter: impl Into<String>) -> Self {
        Chip {
            name: name.into(),
            adapter: adapter.into(),
            features: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Orders features the way libsensors does: by kind, then by index.
    pub fn sort(&mut self) {
        self.features.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| numeric_suffix(&a.name).cmp(&numeric_suffix(&b.name)))
        });
    }
}

fn numeric_suffix(name: &str) -> u64 {
    let digits: String = name.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}

/// Matches a chip against a user supplied chip name pattern, à la sensors(1).
///
/// Accepts a bare prefix (`coretemp`), a full name (`coretemp-isa-0000`) and
/// `*` wildcards for the bus/address parts (`coretemp-*-*`).
pub fn chip_matches(chip: &str, pattern: &str) -> bool {
    if chip == pattern {
        return true;
    }
    let cparts: Vec<&str> = chip.split('-').collect();
    let pparts: Vec<&str> = pattern.split('-').collect();
    if pparts.len() == 1 {
        return cparts.first() == Some(&pattern) || pattern == "*";
    }
    if pparts.len() != cparts.len() {
        return false;
    }
    cparts
        .iter()
        .zip(pparts.iter())
        .all(|(c, p)| *p == "*" || c == p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_names_follow_libsensors() {
        let mut f = Feature::new(Kind::Temp, 1, "Core 0");
        f.push("input", 45.0).push("crit", 100.0);
        assert_eq!(f.name, "temp1");
        assert_eq!(f.subs[0].name, "temp1_input");
        assert_eq!(f.input(), Some(45.0));
        assert_eq!(f.get("crit"), Some(100.0));
        assert_eq!(f.get("max"), None);
    }

    #[test]
    fn chip_patterns() {
        assert!(chip_matches("coretemp-isa-0000", "coretemp"));
        assert!(chip_matches("coretemp-isa-0000", "coretemp-isa-0000"));
        assert!(chip_matches("coretemp-isa-0000", "coretemp-*-*"));
        assert!(chip_matches("coretemp-isa-0000", "*"));
        assert!(!chip_matches("coretemp-isa-0000", "applesmc"));
        assert!(!chip_matches("coretemp-isa-0000", "coretemp-pci-*"));
    }

    #[test]
    fn features_sort_by_kind_then_index() {
        let mut c = Chip::new("x-isa-0000", "ISA adapter");
        c.features.push(Feature::new(Kind::Temp, 10, "t10"));
        c.features.push(Feature::new(Kind::Fan, 2, "f2"));
        c.features.push(Feature::new(Kind::Temp, 2, "t2"));
        c.sort();
        let names: Vec<&str> = c.features.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["fan2", "temp2", "temp10"]);
    }
}
