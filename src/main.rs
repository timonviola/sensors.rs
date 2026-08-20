//! sensors(1) - print hardware monitoring sensor readings.
//!
//! A dependency-free Rust implementation that works the same way on macOS
//! (SMC + Apple HID sensors) and Linux (sysfs hwmon).

// Apple naming helpers are only consumed by the macOS backend, but stay
// compiled (and unit-tested) everywhere.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod apple_map;
mod model;
mod output;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

use std::io::Write;
use std::process::ExitCode;

use model::{chip_matches, Chip};
use output::{Format, Options};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "\
Usage: sensors [OPTION]... [CHIP]...

Print sensor readings (temperatures, fan speeds, voltages, power).

Options:
  -A, --no-adapter   do not print the adapter for each chip
  -f, --fahrenheit   show temperatures in degrees Fahrenheit
  -j, --json         output readings as JSON
  -u, --raw          raw output (one sub-feature per line)
  -c FILE            ignored, accepted for lm-sensors compatibility
  -h, --help         display this help and exit
  -v, --version      display version information and exit

CHIP may be a chip name (`coretemp`), a full name (`coretemp-isa-0000`) or a
pattern with `*` wildcards (`coretemp-*-*`).
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (opts, chip_filters) = match parse_args(&args) {
        Ok(Parsed::Run(o, f)) => (o, f),
        Ok(Parsed::Help) => {
            print!("{}", USAGE);
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Version) => {
            println!("sensors version {} (sensors.rs)", VERSION);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("sensors: {}\n\n{}", e, USAGE);
            return ExitCode::from(1);
        }
    };

    let mut chips = collect();
    if !chip_filters.is_empty() {
        chips.retain(|c| chip_filters.iter().any(|p| chip_matches(&c.name, p)));
    }
    chips.retain(|c| !c.is_empty());

    if chips.is_empty() {
        eprintln!("sensors: no sensors found");
        return ExitCode::from(1);
    }

    let text = output::render(&chips, &opts);
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    // Ignore EPIPE, which happens routinely when piping into head/awk.
    let _ = lock.write_all(text.as_bytes());
    let _ = lock.flush();
    ExitCode::SUCCESS
}

fn collect() -> Vec<Chip> {
    #[cfg(target_os = "macos")]
    {
        macos::collect()
    }
    #[cfg(target_os = "linux")]
    {
        linux::collect()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Vec::new()
    }
}

enum Parsed {
    Run(Options, Vec<String>),
    Help,
    Version,
}

fn parse_args(args: &[String]) -> Result<Parsed, String> {
    let mut opts = Options::default();
    let mut chips = Vec::new();
    let mut iter = args.iter().peekable();
    let mut only_positional = false;

    while let Some(arg) = iter.next() {
        if only_positional || !arg.starts_with('-') || arg == "-" {
            chips.push(arg.clone());
            continue;
        }
        match arg.as_str() {
            "--" => only_positional = true,
            "-h" | "--help" => return Ok(Parsed::Help),
            "-v" | "--version" => return Ok(Parsed::Version),
            "-A" | "--no-adapter" => opts.show_adapter = false,
            "-f" | "--fahrenheit" => opts.fahrenheit = true,
            "-j" | "--json" => opts.format = Format::Json,
            "-u" | "--raw" => opts.format = Format::Raw,
            "-c" | "--config-file" => {
                iter.next()
                    .ok_or_else(|| "option '-c' requires an argument".to_string())?;
            }
            other if other.starts_with("--") => {
                return Err(format!("unrecognized option '{}'", other))
            }
            // Support bundled short flags such as `-fA`.
            other => {
                for c in other.chars().skip(1) {
                    match c {
                        'A' => opts.show_adapter = false,
                        'f' => opts.fahrenheit = true,
                        'j' => opts.format = Format::Json,
                        'u' => opts.format = Format::Raw,
                        'h' => return Ok(Parsed::Help),
                        'v' => return Ok(Parsed::Version),
                        _ => return Err(format!("invalid option -- '{}'", c)),
                    }
                }
            }
        }
    }
    Ok(Parsed::Run(opts, chips))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Parsed, String> {
        parse_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    fn opts_of(p: Parsed) -> (Options, Vec<String>) {
        match p {
            Parsed::Run(o, c) => (o, c),
            _ => panic!("expected run"),
        }
    }

    #[test]
    fn defaults() {
        let (o, chips) = opts_of(run(&[]).unwrap());
        assert!(o.show_adapter);
        assert!(!o.fahrenheit);
        assert!(o.format == Format::Default);
        assert!(chips.is_empty());
    }

    #[test]
    fn flags_and_chip_names() {
        let (o, chips) = opts_of(run(&["-f", "-A", "coretemp-isa-0000"]).unwrap());
        assert!(o.fahrenheit);
        assert!(!o.show_adapter);
        assert_eq!(chips, ["coretemp-isa-0000"]);
    }

    #[test]
    fn bundled_short_flags() {
        let (o, _) = opts_of(run(&["-fA"]).unwrap());
        assert!(o.fahrenheit && !o.show_adapter);
    }

    #[test]
    fn config_file_is_accepted_and_ignored() {
        let (_, chips) = opts_of(run(&["-c", "/etc/sensors3.conf", "applesmc"]).unwrap());
        assert_eq!(chips, ["applesmc"]);
        assert!(run(&["-c"]).is_err());
    }

    #[test]
    fn help_and_version() {
        assert!(matches!(run(&["--help"]).unwrap(), Parsed::Help));
        assert!(matches!(run(&["-v"]).unwrap(), Parsed::Version));
    }

    #[test]
    fn unknown_options_error() {
        assert!(run(&["--nope"]).is_err());
        assert!(run(&["-z"]).is_err());
    }

    #[test]
    fn double_dash_stops_option_parsing() {
        let (_, chips) = opts_of(run(&["--", "-weird-chip"]).unwrap());
        assert_eq!(chips, ["-weird-chip"]);
    }
}
