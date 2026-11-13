mod acpi;
mod compat;
mod config;
mod download;
mod hardware;
mod kexts;

use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = PathBuf::from(arg_value(&args, "--out").unwrap_or_else(|| "EFI-Output".to_string()));
    let dvmt_60 = args.iter().any(|a| a == "--dvmt-60");

    let hw = hardware::detect()?;
    compat::check(&hw)?;
    println!("CPU: {} (generation {})", hw.cpu_name, hw.generation);
    println!("Form factor: {}", if hw.is_laptop { "laptop" } else { "desktop" });
    for device in hw.devices.iter().filter(|d| d.class == hardware::PciClass::Display) {
        println!("GPU: {:04x}:{:04x}", device.vendor, device.device);
    }

    let macos = compat::choose(&hw, arg_value(&args, "--macos"))?;
    let xe = hw.xe_igpu().is_some();

    let work = std::env::temp_dir().join("opcore-simplify");
    if work.exists() {
        std::fs::remove_dir_all(&work)?;
    }
    std::fs::create_dir_all(&work)?;

    let efi = out.join("EFI");
    if efi.exists() {
        std::fs::remove_dir_all(&efi)?;
    }
    std::fs::create_dir_all(&out)?;

    let opencore_url = download::release_asset("acidanthera/OpenCorePkg", "OpenCore-", "release")?;
    download::extract_zip(&opencore_url, &work.join("oc"))?;
    let oc_root = work.join("oc");
    let base_efi = download::find(&oc_root.join("X64"), "EFI")
        .or_else(|| download::find(&oc_root, "EFI"))
        .ok_or_else(|| anyhow!("EFI folder not found in OpenCore package"))?;
    let sample_plist = download::find(&oc_root, "Sample.plist")
        .ok_or_else(|| anyhow!("Sample.plist not found in OpenCore package"))?;
    let acpi_samples = download::find(&oc_root, "Binaries")
        .ok_or_else(|| anyhow!("ACPI sample binaries not found in OpenCore package"))?;
    kexts::copy_dir(&base_efi, &efi)?;

    let oc = efi.join("OC");
    for folder in ["Drivers", "Tools", "ACPI", "Kexts"] {
        let path = oc.join(folder);
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        std::fs::create_dir_all(&path)?;
    }

    let runtime = download::find(&base_efi.join("OC/Drivers"), "OpenRuntime.efi")
        .ok_or_else(|| anyhow!("OpenRuntime.efi not found in OpenCore package"))?;
    std::fs::copy(&runtime, oc.join("Drivers/OpenRuntime.efi")).context("copying OpenRuntime.efi")?;
    std::fs::write(
        oc.join("Drivers/HfsPlus.efi"),
        download::fetch("https://github.com/acidanthera/OcBinaryData/raw/master/Drivers/HfsPlus.efi")?,
    )?;
    let drivers = vec!["HfsPlus.efi".to_string(), "OpenRuntime.efi".to_string()];

    let acpi_files = acpi::install(&acpi_samples, &oc.join("ACPI"), &hw)?;
    let kext_entries = kexts::fetch(&hw, xe, &work, &oc.join("Kexts"))?;

    let options = config::Options { xe, dvmt_60 };
    let plist = config::build(
        &sample_plist,
        &hw,
        macos,
        &options,
        &kext_entries,
        &acpi_files,
        &drivers,
    )?;
    plist::to_file_xml(oc.join("config.plist"), &plist).context("writing config.plist")?;

    println!("EFI written to {}", efi.display());
    if xe {
        println!("After first boot install the latest package from https://github.com/b00t0x/WhateverXe-Graphics/releases and restart");
    }
    Ok(())
}
