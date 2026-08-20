//! Output formatting, byte-for-byte compatible with lm-sensors' `sensors(1)`.
//!
//! libsensors aligns values on `max_label_len + 2` columns and prints numbers
//! with fixed widths (`%+6.1f` for temperatures, `%4.0f` for fans, ...). Tools
//! that scrape `sensors` output - such as the tmux-cpu plugin - depend on this,
//! so the layout is reproduced exactly.

use crate::model::{Chip, Feature, Kind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Default,
    Raw,
    Json,
}

pub struct Options {
    pub format: Format,
    pub fahrenheit: bool,
    pub show_adapter: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            format: Format::Default,
            fahrenheit: false,
            show_adapter: true,
        }
    }
}

pub fn render(chips: &[Chip], opts: &Options) -> String {
    match opts.format {
        Format::Json => render_json(chips, opts),
        Format::Raw => render_plain(chips, opts, true),
        Format::Default => render_plain(chips, opts, false),
    }
}

fn render_plain(chips: &[Chip], opts: &Options, raw: bool) -> String {
    let mut out = String::new();
    for chip in chips {
        out.push_str(&chip.name);
        out.push('\n');
        if opts.show_adapter {
            out.push_str(&format!("Adapter: {}\n", chip.adapter));
        }
        if raw {
            for f in &chip.features {
                out.push_str(&format!("{}:\n", f.name));
                for s in &f.subs {
                    out.push_str(&format!("  {}: {:.3}\n", s.name, convert(s, f.kind, opts)));
                }
            }
        } else {
            let width = chip
                .features
                .iter()
                .map(|f| f.label.chars().count())
                .max()
                .unwrap_or(0);
            for f in &chip.features {
                out.push_str(&print_label(&f.label, width));
                out.push_str(&print_value(f, opts));
                out.push('\n');
            }
        }
        out.push('\n');
    }
    out
}

/// `printf("%s:%*s", label, space - len, "")` from libsensors' print_label().
fn print_label(label: &str, max_label: usize) -> String {
    let pad = (max_label + 1).saturating_sub(label.chars().count());
    format!("{}:{}", label, " ".repeat(pad))
}

fn deg(opts: &Options) -> &'static str {
    if opts.fahrenheit {
        "\u{b0}F"
    } else {
        "\u{b0}C"
    }
}

fn to_f(c: f64) -> f64 {
    c * 9.0 / 5.0 + 32.0
}

/// Applies unit conversion (currently only Celsius -> Fahrenheit) to a value.
fn convert(sub: &crate::model::Subfeature, kind: Kind, opts: &Options) -> f64 {
    if kind == Kind::Temp && opts.fahrenheit && !sub.name.ends_with("_alarm") {
        to_f(sub.value)
    } else {
        sub.value
    }
}

fn fmt_temp(v: f64, opts: &Options) -> String {
    let v = if opts.fahrenheit { to_f(v) } else { v };
    format!("{:>+6.1}{}", v, deg(opts))
}

fn print_value(f: &Feature, opts: &Options) -> String {
    let input = match f.input() {
        Some(v) => v,
        None => {
            // Feature without an input value: print whatever we have.
            return f
                .subs
                .first()
                .map(|s| format!("{:.1}", s.value))
                .unwrap_or_else(|| "N/A".to_string());
        }
    };

    let mut s = match f.kind {
        Kind::Temp => fmt_temp(input, opts),
        Kind::Fan => format!("{:>4.0} RPM", input),
        Kind::Voltage => format!("{:>+6.2} V", input),
        Kind::Power => format_scaled(input, "W"),
        Kind::Energy => format_scaled(input, "J"),
        Kind::Current => format!("{:>+6.2} A", input),
        Kind::Humidity => format!("{:>6.1} %RH", input),
        Kind::Other => format!("{:>6.1}", input),
    };

    let mut extra: Vec<String> = Vec::new();
    match f.kind {
        Kind::Temp => {
            for (suffix, name) in [
                ("min", "low"),
                ("max", "high"),
                ("crit", "crit"),
                ("emergency", "emerg"),
            ] {
                if let Some(v) = f.get(suffix) {
                    extra.push(format!("{} = {}", name, fmt_temp(v, opts).trim_start()));
                }
            }
        }
        Kind::Fan => {
            if let Some(v) = f.get("min") {
                extra.push(format!("min = {:.0} RPM", v));
            }
            if let Some(v) = f.get("max") {
                extra.push(format!("max = {:.0} RPM", v));
            }
        }
        Kind::Voltage => {
            if let Some(v) = f.get("min") {
                extra.push(format!("min = {:+.2} V", v));
            }
            if let Some(v) = f.get("max") {
                extra.push(format!("max = {:+.2} V", v));
            }
        }
        Kind::Power => {
            if let Some(v) = f.get("max") {
                extra.push(format!("max = {}", format_scaled(v, "W").trim_start()));
            }
        }
        _ => {}
    }
    if !extra.is_empty() {
        s.push_str(&format!("  ({})", extra.join(", ")));
    }
    if f.get("alarm").unwrap_or(0.0) != 0.0 {
        s.push_str("  ALARM");
    }
    s
}

/// libsensors scales small power/energy readings down to milli units.
fn format_scaled(v: f64, unit: &str) -> String {
    if v != 0.0 && v.abs() < 1.0 {
        format!("{:>6.2} m{}", v * 1000.0, unit)
    } else {
        format!("{:>6.2} {}", v, unit)
    }
}

fn render_json(chips: &[Chip], opts: &Options) -> String {
    let mut out = String::from("{\n");
    for (ci, chip) in chips.iter().enumerate() {
        out.push_str(&format!("   {}:{{\n", quote(&chip.name)));
        let mut entries: Vec<String> = Vec::new();
        if opts.show_adapter {
            entries.push(format!("      \"Adapter\": {}", quote(&chip.adapter)));
        }
        for f in &chip.features {
            let subs: Vec<String> = f
                .subs
                .iter()
                .map(|s| {
                    format!(
                        "         {}: {:.3}",
                        quote(&s.name),
                        convert(s, f.kind, opts)
                    )
                })
                .collect();
            entries.push(format!(
                "      {}:{{\n{}\n      }}",
                quote(&f.label),
                subs.join(",\n")
            ));
        }
        out.push_str(&entries.join(",\n"));
        out.push_str("\n   }");
        if ci + 1 < chips.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

fn quote(s: &str) -> String {
    let mut q = String::with_capacity(s.len() + 2);
    q.push('"');
    for c in s.chars() {
        match c {
            '"' => q.push_str("\\\""),
            '\\' => q.push_str("\\\\"),
            '\n' => q.push_str("\\n"),
            '\t' => q.push_str("\\t"),
            c if (c as u32) < 0x20 => q.push_str(&format!("\\u{:04x}", c as u32)),
            c => q.push(c),
        }
    }
    q.push('"');
    q
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Feature;

    fn coretemp() -> Chip {
        let mut chip = Chip::new("coretemp-isa-0000", "ISA adapter");
        let mut pkg = Feature::new(Kind::Temp, 1, "Package id 0");
        pkg.push("input", 47.0)
            .push("max", 100.0)
            .push("crit", 100.0);
        let mut c0 = Feature::new(Kind::Temp, 2, "Core 0");
        c0.push("input", 45.0)
            .push("max", 100.0)
            .push("crit", 100.0);
        chip.features.push(pkg);
        chip.features.push(c0);
        chip
    }

    #[test]
    fn layout_matches_lm_sensors() {
        let out = render(&[coretemp()], &Options::default());
        let expected = "coretemp-isa-0000\n\
                        Adapter: ISA adapter\n\
                        Package id 0:  +47.0\u{b0}C  (high = +100.0\u{b0}C, crit = +100.0\u{b0}C)\n\
                        Core 0:        +45.0\u{b0}C  (high = +100.0\u{b0}C, crit = +100.0\u{b0}C)\n\n";
        assert_eq!(out, expected);
    }

    /// The tmux-cpu plugin runs: awk '/^Core [0-9]+/ {gsub("[^0-9.]","",$3); sum+=$3; n+=1}'
    #[test]
    fn tmux_cpu_can_parse_core_lines() {
        let out = render(&[coretemp()], &Options::default());
        let mut sum = 0.0;
        let mut n = 0;
        for line in out.lines() {
            if line.starts_with("Core ") && line.as_bytes()[5].is_ascii_digit() {
                let f3: String = line.split_whitespace().nth(2).unwrap().to_string();
                let cleaned: String = f3
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                sum += cleaned.parse::<f64>().unwrap();
                n += 1;
            }
        }
        assert_eq!(n, 1);
        assert_eq!(sum, 45.0);
    }

    #[test]
    fn fahrenheit_conversion() {
        let opts = Options {
            fahrenheit: true,
            ..Default::default()
        };
        let out = render(&[coretemp()], &opts);
        assert!(out.contains("Core 0:       +113.0\u{b0}F"), "{}", out);
    }

    #[test]
    fn raw_output() {
        let opts = Options {
            format: Format::Raw,
            ..Default::default()
        };
        let out = render(&[coretemp()], &opts);
        assert!(out.contains("temp2:\n  temp2_input: 45.000\n"), "{}", out);
    }

    #[test]
    fn json_output_is_wellformed() {
        let opts = Options {
            format: Format::Json,
            ..Default::default()
        };
        let out = render(&[coretemp()], &opts);
        assert!(out.contains("\"coretemp-isa-0000\":{"), "{}", out);
        assert!(out.contains("\"Core 0\":{"), "{}", out);
        assert!(out.contains("\"temp2_input\": 45.000"), "{}", out);
        assert_eq!(out.matches('{').count(), out.matches('}').count());
    }

    #[test]
    fn fan_and_power_formatting() {
        let mut chip = Chip::new("applesmc-isa-0300", "SMC");
        let mut fan = Feature::new(Kind::Fan, 1, "Fan 1");
        fan.push("input", 1234.0)
            .push("min", 1200.0)
            .push("max", 5500.0);
        let mut pwr = Feature::new(Kind::Power, 1, "CPU Power");
        pwr.push("input", 3.5);
        let mut small = Feature::new(Kind::Power, 2, "GPU Power");
        small.push("input", 0.25);
        chip.features.push(fan);
        chip.features.push(pwr);
        chip.features.push(small);
        let out = render(&[chip], &Options::default());
        assert!(
            out.contains("Fan 1:     1234 RPM  (min = 1200 RPM, max = 5500 RPM)"),
            "{}",
            out
        );
        assert!(out.contains("CPU Power:   3.50 W"), "{}", out);
        assert!(out.contains("GPU Power: 250.00 mW"), "{}", out);
    }

    #[test]
    fn no_adapter_flag() {
        let opts = Options {
            show_adapter: false,
            ..Default::default()
        };
        let out = render(&[coretemp()], &opts);
        assert!(!out.contains("Adapter:"));
    }
}
