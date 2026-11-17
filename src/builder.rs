use crate::acpi;
use crate::compat::{MacOs, Report};
use crate::config;
use crate::download;
use crate::hardware::{Hardware, PciClass};
use crate::kexts;
use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;

/// Everything the user can customize before building.
pub struct State {
    pub hw: Hardware,
    pub report: Report,
    pub macos: &'static MacOs,
    pub kexts: Vec<kexts::Choice>,
    pub acpi: Vec<acpi::Patch>,
    pub smbios: String,
    pub dvmt_60: bool,
    pub out: PathBuf,
}

impl State {
    pub fn new(hw: Hardware, report: Report, macos: &'static MacOs, out: PathBuf, dvmt_60: bool) -> Self {
        let kexts = kexts::plan(&hw, &report, macos.major);
        let acpi = acpi::patches(&hw);
        let smbios = config::default_model(&hw, macos).to_string();
        Self { hw, report, macos, kexts, acpi, smbios, dvmt_60, out }
    }

    pub fn set_macos(&mut self, macos: &'static MacOs) {
        self.macos = macos;
        self.kexts = kexts::plan(&self.hw, &self.report, macos.major);
        self.smbios = config::default_model(&self.hw, macos).to_string();
    }

    pub fn xe(&self) -> bool {
        self.hw.xe_igpu().is_some()
    }
}

pub fn build_efi(state: &State) -> Result<PathBuf> {
    let work = std::env::temp_dir().join("opcore-builder");
    if work.exists() {
        std::fs::remove_dir_all(&work)?;
    }
    std::fs::create_dir_all(&work)?;

    let efi = state.out.join("EFI");
    if efi.exists() {
        std::fs::remove_dir_all(&efi)?;
    }
    std::fs::create_dir_all(&state.out)?;

    println!("Downloading OpenCore ...");
    let opencore_url = download::release_asset("acidanthera/OpenCorePkg", "OpenCore-", "release")?;
    let oc_root = work.join("oc");
    download::extract_zip(&opencore_url, &oc_root)?;
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
    println!("Downloading HfsPlus.efi ...");
    std::fs::write(
        oc.join("Drivers/HfsPlus.efi"),
        download::fetch("https://github.com/acidanthera/OcBinaryData/raw/master/Drivers/HfsPlus.efi")?,
    )?;
    let drivers = vec!["HfsPlus.efi".to_string(), "OpenRuntime.efi".to_string()];

    let acpi_files = acpi::install(&acpi_samples, &oc.join("ACPI"), &state.acpi)?;
    let kext_entries = kexts::fetch(&state.kexts, &work, &oc.join("Kexts"))?;

    let options = config::Options {
        xe: state.xe(),
        dvmt_60: state.dvmt_60,
        smbios: state.smbios.clone(),
    };
    let plist = config::build(&sample_plist, &state.hw, &options, &kext_entries, &acpi_files, &drivers)?;
    plist::to_file_xml(oc.join("config.plist"), &plist).context("writing config.plist")?;
    Ok(efi)
}

pub fn checklist(state: &State) -> Vec<String> {
    let hw = &state.hw;
    let mut list = vec![
        "BIOS: disable Secure Boot and boot in UEFI mode (CSM off)".to_string(),
        "BIOS: set the SATA/storage mode to AHCI (Intel RST/VMD RAID is not supported)".to_string(),
        "BIOS: disable CFG Lock if it is exposed (AppleXcpmCfgLock patch is enabled otherwise)".to_string(),
        "USB: map your ports with USBToolBox after the first boot (XhciPortLimit is off)".to_string(),
        "Serial, MLB, ROM and UUID are randomly generated: check the serial on Apple's coverage page".to_string(),
    ];
    if state.xe() {
        list.push("Xe graphics: set DVMT pre-alloc to 64MB or more, or build with --dvmt-60".to_string());
        list.push(
            "Xe graphics: after the first boot install the latest package from https://github.com/b00t0x/WhateverXe-Graphics/releases and restart"
                .to_string(),
        );
    }
    if hw.devices.iter().any(|d| d.class == PciClass::Wireless && d.vendor == 0x8086) {
        list.push("Intel Wi-Fi: install the HeliPort app to connect to networks".to_string());
    }
    if hw.i2c {
        list.push("I2C trackpad: VoodooI2C may need GPIO pinning or ACPI patches to work".to_string());
    }
    list
}

pub fn run_build(state: &State) -> Result<()> {
    let efi = build_efi(state)?;
    println!("\nEFI written to {}", efi.display());
    println!("\nBefore you boot:");
    for line in checklist(state) {
        println!("  - {line}");
    }
    Ok(())
}
