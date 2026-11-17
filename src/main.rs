mod acpi;
mod builder;
mod compat;
mod config;
mod download;
mod hardware;
mod kexts;
mod ui;

use anyhow::{bail, Result};
use std::path::PathBuf;

const USAGE: &str = "opcore-builder - generate an OpenCore EFI for this machine

usage: opcore-builder [options]
  (no options)       interactive menu
  --yes              skip the menu and build with the recommended settings
  --macos <version>  macOS version, e.g. Sonoma or 14
  --out <folder>     output folder (default: EFI-Output)
  --dvmt-60          Tiger Lake Xe: patch DVMT pre-alloc to 60MB
  --help             show this text

Set GITHUB_TOKEN to use the GitHub API with a higher rate limit.";

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let out = PathBuf::from(arg_value(&args, "--out").unwrap_or_else(|| "EFI-Output".to_string()));
    let dvmt_60 = args.iter().any(|a| a == "--dvmt-60");
    let non_interactive = args.iter().any(|a| a == "--yes");

    let hw = hardware::detect()?;
    let report = compat::analyze(&hw);
    let macos = match arg_value(&args, "--macos") {
        Some(value) => compat::parse_version(&report, &value)?,
        None => compat::default_version(&report),
    };
    let mut state = builder::State::new(hw, report, macos, out, dvmt_60);

    if non_interactive {
        ui::print_report(&state);
        ui::print_compat(&state);
        if let Some(reason) = &state.report.blocked {
            bail!("this hardware cannot be built: {reason}");
        }
        println!("\nTarget: macOS {} ({}), SMBIOS {}\n", state.macos.name, state.macos.major, state.smbios);
        return builder::run_build(&state);
    }
    ui::run(&mut state)
}
