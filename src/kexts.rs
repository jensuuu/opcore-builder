use crate::download;
use crate::hardware::{Hardware, PciClass};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub struct KextEntry {
    pub bundle: String,
    pub exec: String,
    pub plist: String,
}

enum Origin {
    Github(&'static str, &'static str, &'static str),
    Direct(&'static str),
}

struct Source {
    origin: Origin,
    wanted: Vec<&'static str>,
}

fn github(repo: &'static str, prefix: &'static str, contains: &'static str, wanted: &[&'static str]) -> Source {
    Source {
        origin: Origin::Github(repo, prefix, contains),
        wanted: wanted.to_vec(),
    }
}

fn plan(hw: &Hardware, xe: bool) -> Vec<Source> {
    let mut sources = vec![github("acidanthera/Lilu", "Lilu-", "release", &["Lilu.kext"])];

    let mut smc = vec!["VirtualSMC.kext", "SMCProcessor.kext"];
    smc.push(if hw.is_laptop { "SMCBatteryManager.kext" } else { "SMCSuperIO.kext" });
    sources.push(github("acidanthera/VirtualSMC", "VirtualSMC-", "release", &smc));

    if xe {
        sources.push(Source {
            origin: Origin::Github("b00t0x/WhateverXe", "", "release"),
            wanted: vec![],
        });
        sources.push(Source {
            origin: Origin::Direct(
                "https://github.com/b00t0x/WhateverXe-Graphics/raw/main/extras/AMFIPass-v1.4.1-Mod-RELEASE.zip",
            ),
            wanted: vec![],
        });
    } else if hw.has(PciClass::Display) {
        sources.push(github(
            "acidanthera/WhateverGreen",
            "WhateverGreen-",
            "release",
            &["WhateverGreen.kext"],
        ));
    }

    if hw.has(PciClass::Audio) {
        sources.push(github("acidanthera/AppleALC", "AppleALC-", "release", &["AppleALC.kext"]));
    }
    sources.push(github(
        "acidanthera/RestrictEvents",
        "RestrictEvents-",
        "release",
        &["RestrictEvents.kext"],
    ));
    if hw.has(PciClass::Nvme) {
        sources.push(github("acidanthera/NVMeFix", "NVMeFix-", "release", &["NVMeFix.kext"]));
    }
    if hw.is_laptop {
        sources.push(github("1Revenger1/ECEnabler", "ECEnabler", "", &["ECEnabler.kext"]));
        sources.push(github(
            "acidanthera/VoodooPS2",
            "VoodooPS2Controller-",
            "release",
            &["VoodooPS2Controller.kext"],
        ));
        sources.push(github(
            "VoodooI2C/VoodooI2C",
            "VoodooI2C-",
            "",
            &["VoodooI2C.kext", "VoodooI2CHID.kext"],
        ));
    }

    const I225: [u16; 6] = [0x15F2, 0x15F3, 0x3100, 0x125B, 0x125C, 0x125D];
    for device in &hw.devices {
        match (device.class, device.vendor, device.device) {
            (PciClass::Ethernet, 0x8086, id) if !I225.contains(&id) => {
                sources.push(github("acidanthera/IntelMausi", "IntelMausi-", "release", &["IntelMausi.kext"]));
            }
            (PciClass::Ethernet, 0x10EC, 0x8168 | 0x8111) => {
                sources.push(github(
                    "Mieze/RTL8111_driver_for_OS_X",
                    "RealtekRTL8111-",
                    "",
                    &["RealtekRTL8111.kext"],
                ));
            }
            (PciClass::Ethernet, 0x10EC, 0x8125) => {
                sources.push(github(
                    "Mieze/LucyRTL8125Ethernet",
                    "LucyRTL8125Ethernet-",
                    "",
                    &["LucyRTL8125Ethernet.kext"],
                ));
            }
            (PciClass::Wireless, 0x8086, _) => {
                sources.push(github("OpenIntelWireless/itlwm", "itlwm_", "", &["itlwm.kext"]));
            }
            (PciClass::Wireless, 0x14E4, _) => {
                sources.push(github(
                    "acidanthera/AirportBrcmFixup",
                    "AirportBrcmFixup-",
                    "release",
                    &["AirportBrcmFixup.kext"],
                ));
            }
            _ => {}
        }
    }
    sources
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

pub fn fetch(hw: &Hardware, xe: bool, work: &Path, destination: &Path) -> Result<Vec<KextEntry>> {
    std::fs::create_dir_all(destination)?;
    let mut names: Vec<String> = Vec::new();
    let mut seen = HashSet::new();

    for (index, source) in plan(hw, xe).into_iter().enumerate() {
        let key = match &source.origin {
            Origin::Github(repo, _, _) => repo.to_string(),
            Origin::Direct(url) => url.to_string(),
        };
        if !seen.insert(key.clone()) {
            continue;
        }

        let extract = work.join(format!("kext-{index}"));
        let result = (|| -> Result<()> {
            let url = match &source.origin {
                Origin::Github(repo, prefix, contains) => download::release_asset(repo, prefix, contains)?,
                Origin::Direct(url) => url.to_string(),
            };
            download::extract_zip(&url, &extract)
        })();
        if let Err(error) = result {
            eprintln!("skipped {key}: {error}");
            continue;
        }

        let mut found = Vec::new();
        if extract.is_dir() {
            top_level_kexts(&extract, &mut found)?;
        }
        found.sort_by_key(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            source.wanted.iter().position(|w| *w == name).unwrap_or(usize::MAX)
        });

        for path in found {
            let relative = path.strip_prefix(&extract).unwrap_or(&path);
            if relative.components().any(|c| {
                let part = c.as_os_str().to_string_lossy().to_lowercase();
                part == "debug"
            }) {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !source.wanted.is_empty() && !source.wanted.iter().any(|w| *w == name) {
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
    Ok(entries)
}
