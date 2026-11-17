use crate::compat::Report;
use crate::download;
use crate::hardware::Hardware;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct KextEntry {
    pub bundle: String,
    pub exec: String,
    pub plist: String,
}

/// A kext we know how to download.
pub struct Def {
    pub id: &'static str,
    pub desc: &'static str,
    /// GitHub "owner/name" (empty when `direct` is used)
    pub repo: &'static str,
    /// release asset name must start with this
    pub asset: &'static str,
    /// ...and contain this (case-insensitive)
    pub contains: &'static str,
    pub direct: &'static str,
    /// bundles to copy out of the zip (empty = every non-debug kext)
    pub bundles: &'static [&'static str],
    /// macOS major versions this kext is used on
    pub min: u32,
    pub max: u32,
}

const fn gh(
    id: &'static str,
    desc: &'static str,
    repo: &'static str,
    asset: &'static str,
    contains: &'static str,
    bundles: &'static [&'static str],
    min: u32,
    max: u32,
) -> Def {
    Def { id, desc, repo, asset, contains, direct: "", bundles, min, max }
}

/// Order matters: it is the load order written to config.plist.
pub static CATALOG: &[Def] = &[
    gh("Lilu", "Patch engine required by most kexts", "acidanthera/Lilu", "Lilu-", "release", &["Lilu.kext"], 11, 26),
    gh("VirtualSMC", "SMC emulator and CPU sensors", "acidanthera/VirtualSMC", "VirtualSMC-", "release", &["VirtualSMC.kext", "SMCProcessor.kext"], 11, 26),
    gh("SMCBatteryManager", "Battery status (laptops)", "acidanthera/VirtualSMC", "VirtualSMC-", "release", &["SMCBatteryManager.kext"], 11, 26),
    gh("SMCSuperIO", "Fan and sensor chips (desktops)", "acidanthera/VirtualSMC", "VirtualSMC-", "release", &["SMCSuperIO.kext"], 11, 26),
    gh("WhateverGreen", "Graphics patches (Intel, AMD, NVIDIA disable)", "acidanthera/WhateverGreen", "WhateverGreen-", "release", &["WhateverGreen.kext"], 11, 26),
    gh("WhateverXe", "Tiger Lake Xe graphics", "b00t0x/WhateverXe", "", "release", &[], 11, 26),
    Def {
        id: "AMFIPass",
        desc: "Lets the Xe graphics stack load with AMFI enabled",
        repo: "",
        asset: "",
        contains: "",
        direct: "https://github.com/b00t0x/WhateverXe-Graphics/raw/main/extras/AMFIPass-v1.4.1-Mod-RELEASE.zip",
        bundles: &[],
        min: 11,
        max: 26,
    },
    gh("AppleALC", "Native audio codec support", "acidanthera/AppleALC", "AppleALC-", "release", &["AppleALC.kext"], 11, 26),
    gh("RestrictEvents", "CPU naming and unsupported-Mac fixes", "acidanthera/RestrictEvents", "RestrictEvents-", "release", &["RestrictEvents.kext"], 11, 26),
    gh("NVMeFix", "NVMe power management", "acidanthera/NVMeFix", "NVMeFix-", "release", &["NVMeFix.kext"], 11, 26),
    gh("ECEnabler", "Battery reads from 16-bit embedded controller fields", "1Revenger1/ECEnabler", "ECEnabler", "", &["ECEnabler.kext"], 11, 26),
    gh("VoodooPS2", "PS/2 keyboard, mouse and trackpad", "acidanthera/VoodooPS2", "VoodooPS2Controller-", "release", &["VoodooPS2Controller.kext"], 11, 26),
    gh("VoodooI2C", "I2C HID trackpads and touchscreens", "VoodooI2C/VoodooI2C", "VoodooI2C-", "", &["VoodooI2C.kext", "VoodooI2CHID.kext"], 11, 26),
    gh("IntelMausi", "Intel Ethernet (I217/I218/I219)", "acidanthera/IntelMausi", "IntelMausi-", "release", &["IntelMausi.kext"], 11, 26),
    gh("RealtekRTL8111", "Realtek RTL8111/8168 Ethernet", "Mieze/RTL8111_driver_for_OS_X", "RealtekRTL8111-", "", &["RealtekRTL8111.kext"], 11, 26),
    gh("LucyRTL8125Ethernet", "Realtek RTL8125 2.5G Ethernet", "Mieze/LucyRTL8125Ethernet", "LucyRTL8125Ethernet-", "", &["LucyRTL8125Ethernet.kext"], 11, 26),
    gh("itlwm", "Intel Wi-Fi (connect with HeliPort)", "OpenIntelWireless/itlwm", "itlwm_", "", &["itlwm.kext"], 11, 26),
    gh("AirportBrcmFixup", "Broadcom Wi-Fi patches", "acidanthera/AirportBrcmFixup", "AirportBrcmFixup-", "release", &["AirportBrcmFixup.kext"], 11, 26),
    gh("IntelBluetoothFirmware", "Intel Bluetooth firmware upload", "OpenIntelWireless/IntelBluetoothFirmware", "IntelBluetooth", "", &["IntelBluetoothFirmware.kext"], 11, 26),
    gh("IntelBTPatcher", "Intel Bluetooth fixes", "OpenIntelWireless/IntelBluetoothFirmware", "IntelBluetooth", "", &["IntelBTPatcher.kext"], 11, 26),
    gh("IntelBluetoothInjector", "Intel Bluetooth injector (Big Sur only)", "OpenIntelWireless/IntelBluetoothFirmware", "IntelBluetooth", "", &["IntelBluetoothInjector.kext"], 11, 11),
    gh("BlueToolFixup", "Bluetooth stack fix for third-party cards (Monterey+)", "acidanthera/BrcmPatchRAM", "BrcmPatchRAM-", "release", &["BlueToolFixup.kext"], 12, 26),
    gh("BrcmBluetoothInjector", "Broadcom Bluetooth injector (Big Sur only)", "acidanthera/BrcmPatchRAM", "BrcmPatchRAM-", "release", &["BrcmBluetoothInjector.kext"], 11, 11),
    gh("BrcmFirmwareData", "Broadcom Bluetooth firmware", "acidanthera/BrcmPatchRAM", "BrcmPatchRAM-", "release", &["BrcmFirmwareData.kext"], 11, 26),
    gh("BrcmPatchRAM3", "Broadcom Bluetooth firmware uploader", "acidanthera/BrcmPatchRAM", "BrcmPatchRAM-", "release", &["BrcmPatchRAM3.kext"], 11, 26),
];

pub struct Choice {
    pub def: &'static Def,
    pub enabled: bool,
    pub reason: String,
}

/// Pick kexts for this hardware: the ones the compatibility rows ask for are enabled,
/// every other kext valid for the chosen macOS stays available but off.
pub fn plan(hw: &Hardware, report: &Report, macos: u32) -> Vec<Choice> {
    let mut wanted: Vec<(&'static str, String)> = vec![
        ("Lilu", "required".to_string()),
        ("VirtualSMC", "required".to_string()),
        (
            if hw.is_laptop { "SMCBatteryManager" } else { "SMCSuperIO" },
            if hw.is_laptop { "battery".to_string() } else { "sensors".to_string() },
        ),
        ("RestrictEvents", "recommended".to_string()),
    ];
    if hw.is_laptop {
        wanted.push(("ECEnabler", "laptop".to_string()));
    }
    for row in &report.rows {
        for id in &row.kexts {
            wanted.push((*id, format!("{}: {}", row.category, row.name)));
        }
    }
    CATALOG
        .iter()
        .filter(|d| macos >= d.min && macos <= d.max)
        .map(|def| match wanted.iter().find(|(id, _)| *id == def.id) {
            Some((_, why)) => Choice { def, enabled: true, reason: why.clone() },
            None => Choice { def, enabled: false, reason: String::new() },
        })
        .collect()
}

pub fn copy_dir(source: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        // metadata() follows symlinks; broken links are skipped instead of failing
        let Ok(meta) = std::fs::metadata(entry.path()) else { continue };
        if meta.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn top_level_kexts(root: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("kext") {
            found.push(path);
        } else {
            top_level_kexts(&path, found)?;
        }
    }
    Ok(())
}

fn push_bundle(relative: String, absolute: &Path, entries: &mut Vec<KextEntry>) -> Result<()> {
    let info_path = absolute.join("Contents/Info.plist");
    if !info_path.exists() {
        eprintln!("skipped {relative}: no Contents/Info.plist");
        return Ok(());
    }
    let info = plist::Value::from_file(&info_path)
        .with_context(|| format!("reading {}", info_path.display()))?;
    let exec = info
        .as_dictionary()
        .and_then(|d| d.get("CFBundleExecutable"))
        .and_then(|v| v.as_string())
        .map(|e| format!("Contents/MacOS/{e}"))
        .unwrap_or_default();
    entries.push(KextEntry {
        bundle: relative.clone(),
        exec,
        plist: "Contents/Info.plist".to_string(),
    });

    let plugins = absolute.join("Contents/PlugIns");
    if plugins.is_dir() {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&plugins)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("kext"))
            .collect();
        paths.sort();
        for path in paths {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            push_bundle(format!("{relative}/Contents/PlugIns/{name}"), &path, entries)?;
        }
    }
    Ok(())
}

pub fn fetch(choices: &[Choice], work: &Path, destination: &Path) -> Result<Vec<KextEntry>> {
    std::fs::create_dir_all(destination)?;
    // each release zip is downloaded once even when several kexts come from it
    let mut extracted: HashMap<String, Option<PathBuf>> = HashMap::new();
    let mut names: Vec<String> = Vec::new();
    let mut skipped = 0;

    for choice in choices.iter().filter(|c| c.enabled) {
        let def = choice.def;
        let key = if def.direct.is_empty() {
            format!("{}|{}|{}", def.repo, def.asset, def.contains)
        } else {
            def.direct.to_string()
        };
        if !extracted.contains_key(&key) {
            println!("Downloading {} ...", def.id);
            let dir = work.join(format!("kext-{}", extracted.len()));
            let result = (|| -> Result<()> {
                let url = if def.direct.is_empty() {
                    download::release_asset(def.repo, def.asset, def.contains)?
                } else {
                    def.direct.to_string()
                };
                download::extract_zip(&url, &dir)
            })();
            match result {
                Ok(()) => {
                    extracted.insert(key.clone(), Some(dir));
                }
                Err(error) => {
                    eprintln!("skipped {}: {error}", def.id);
                    extracted.insert(key.clone(), None);
                }
            }
        }
        let Some(Some(dir)) = extracted.get(&key) else {
            skipped += 1;
            continue;
        };

        let mut found = Vec::new();
        if dir.is_dir() {
            top_level_kexts(dir, &mut found)?;
        }
        found.sort_by_key(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            def.bundles.iter().position(|w| *w == name).unwrap_or(usize::MAX)
        });
        for path in found {
            let relative = path.strip_prefix(dir).unwrap_or(&path);
            if relative
                .components()
                .any(|c| c.as_os_str().to_string_lossy().to_lowercase() == "debug")
            {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !def.bundles.is_empty() && !def.bundles.iter().any(|w| *w == name) {
                continue;
            }
            if names.contains(&name) {
                continue;
            }
            copy_dir(&path, &destination.join(&name))?;
            names.push(name);
        }
    }

    let mut entries = Vec::new();
    for name in &names {
        push_bundle(name.clone(), &destination.join(name), &mut entries)?;
    }
    println!("{} kext bundles installed, {} skipped", entries.len(), skipped);
    Ok(entries)
}
