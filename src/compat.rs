use crate::hardware::{vendor_label, Hardware, PciClass};
use anyhow::{anyhow, Result};

pub struct MacOs {
    pub name: &'static str,
    pub major: u32,
}

pub static VERSIONS: [MacOs; 6] = [
    MacOs { name: "Big Sur", major: 11 },
    MacOs { name: "Monterey", major: 12 },
    MacOs { name: "Ventura", major: 13 },
    MacOs { name: "Sonoma", major: 14 },
    MacOs { name: "Sequoia", major: 15 },
    MacOs { name: "Tahoe", major: 26 },
];

pub fn macos_name(major: u32) -> &'static str {
    VERSIONS.iter().find(|m| m.major == major).map(|m| m.name).unwrap_or("?")
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Support {
    Supported,
    Limited,
    Unsupported,
}

/// One line of the compatibility screen.
pub struct Component {
    pub category: &'static str,
    pub name: String,
    pub support: Support,
    pub min: u32,
    pub max: u32,
    pub note: String,
    /// kext ids (see kexts::CATALOG) this component needs
    pub kexts: Vec<&'static str>,
    /// whether min/max narrow the macOS versions offered for the whole machine
    pub limits_range: bool,
    /// an unsupported blocking component makes the build impossible
    pub blocking: bool,
}

pub struct Report {
    pub rows: Vec<Component>,
    pub min: u32,
    pub max: u32,
    pub blocked: Option<String>,
}

fn row(category: &'static str, name: String, support: Support, note: &str, kexts: &[&'static str]) -> Component {
    Component {
        category,
        name,
        support,
        min: 11,
        max: 26,
        note: note.to_string(),
        kexts: kexts.to_vec(),
        limits_range: false,
        blocking: false,
    }
}

const I225: [u16; 6] = [0x15F2, 0x15F3, 0x3100, 0x125B, 0x125C, 0x125D];
const I21X: [u16; 7] = [0x1533, 0x1536, 0x1537, 0x1538, 0x157B, 0x157C, 0x1539];

fn cpu_row(hw: &Hardware) -> Component {
    let name = format!("{} ({})", hw.cpu_name, hw.codename());
    let mut cpu = row("CPU", name, Support::Supported, "", &[]);
    if !hw.intel {
        cpu.support = Support::Unsupported;
        cpu.blocking = true;
        cpu.note = "Only Intel processors are supported by this tool".to_string();
    } else if hw.generation < 6 {
        cpu.support = Support::Unsupported;
        cpu.blocking = true;
        cpu.note = "Too old: a 6th generation (Skylake) or newer CPU is required".to_string();
    } else if hw.generation > 11 {
        cpu.support = Support::Unsupported;
        cpu.blocking = true;
        cpu.note = "Not supported yet: 12th generation and newer need hybrid-core handling".to_string();
    } else {
        cpu.note = format!("Generation {}", hw.generation);
    }
    cpu
}

fn igpu_row(hw: &Hardware) -> Option<Component> {
    let gpu = hw.igpu()?;
    let discrete_present = !hw.discrete().is_empty();
    let mut r = row("iGPU", gpu.name.clone(), Support::Supported, "", &["WhateverGreen"]);
    r.limits_range = true;
    if hw.rocket_lake_igpu() {
        r.support = Support::Unsupported;
        r.kexts = vec![];
        r.blocking = !discrete_present;
        r.limits_range = false;
        r.note = if discrete_present {
            "Rocket Lake graphics have no macOS driver; use the discrete GPU".to_string()
        } else {
            "Rocket Lake graphics have no macOS driver and no discrete GPU was found".to_string()
        };
    } else if hw.xe_igpu().is_some() {
        r.support = Support::Limited;
        r.kexts = vec!["WhateverXe", "AMFIPass"];
        r.note = "No native driver: experimental support via WhateverXe + AMFIPass. Set DVMT pre-alloc to 64MB+ in BIOS or use --dvmt-60".to_string();
    } else if hw.ice_lake {
        r.support = Support::Limited;
        if discrete_present {
            r.limits_range = false;
            r.note = "Ice Lake graphics only work up to Sonoma; use the discrete GPU on newer macOS".to_string();
        } else {
            r.max = 14;
            r.note = "Ice Lake graphics only work up to Sonoma".to_string();
        }
    } else if (6..=10).contains(&hw.generation) {
        r.note = "Native driver with WhateverGreen framebuffer patch".to_string();
    } else {
        r.support = Support::Unsupported;
        r.kexts = vec![];
        r.limits_range = false;
        r.note = "No macOS driver for this iGPU".to_string();
    }
    Some(r)
}

fn discrete_rows(hw: &Hardware, rows: &mut Vec<Component>) {
    let xe = hw.xe_igpu().is_some();
    for gpu in hw.discrete() {
        let id = gpu.device;
        let name = gpu.name.clone();
        let r = match gpu.vendor {
            0x1002 => {
                let ok = (0x67C0..=0x67FF).contains(&id)
                    || (0x6860..=0x687F).contains(&id)
                    || (0x6980..=0x699F).contains(&id)
                    || (0x7310..=0x73FF).contains(&id);
                if ok {
                    row("GPU", name, Support::Supported, "Native AMD driver (Polaris / Vega / Navi)", &["WhateverGreen"])
                } else {
                    row("GPU", name, Support::Unsupported, "Legacy AMD GPU: no driver on current macOS, disable it", &[])
                }
            }
            0x10DE => {
                let kepler = (0x0FC0..=0x0FFF).contains(&id)
                    || (0x1003..=0x103F).contains(&id)
                    || (0x1180..=0x11FF).contains(&id)
                    || (0x1280..=0x12BF).contains(&id);
                if kepler {
                    let mut r = row("GPU", name, Support::Limited, "Kepler: native driver up to Monterey only", &["WhateverGreen"]);
                    r.max = 12;
                    r
                } else {
                    let kexts: &[&'static str] = if xe { &[] } else { &["WhateverGreen"] };
                    row("GPU", name, Support::Unsupported, "No macOS driver (Maxwell and newer); disabled with -wegnoegpu", kexts)
                }
            }
            _ => row("GPU", name, Support::Unsupported, "No macOS driver for this GPU", &[]),
        };
        rows.push(r);
    }
}

fn network_rows(hw: &Hardware, rows: &mut Vec<Component>) {
    for d in hw.devices.iter().filter(|d| d.class == PciClass::Ethernet) {
        let name = d.name.clone();
        rows.push(match (d.vendor, d.device) {
            (0x8086, id) if I225.contains(&id) => row(
                "Ethernet",
                name,
                Support::Limited,
                "I225/I226: no kext added, the native driver may work on Monterey and newer",
                &[],
            ),
            (0x8086, id) if I21X.contains(&id) => {
                row("Ethernet", name, Support::Supported, "Native Apple driver, no kext needed", &[])
            }
            (0x8086, _) => row("Ethernet", name, Support::Supported, "IntelMausi", &["IntelMausi"]),
            (0x10EC, 0x8168 | 0x8111) => {
                row("Ethernet", name, Support::Supported, "RealtekRTL8111", &["RealtekRTL8111"])
            }
            (0x10EC, 0x8125) => {
                row("Ethernet", name, Support::Supported, "LucyRTL8125Ethernet", &["LucyRTL8125Ethernet"])
            }
            _ => row("Ethernet", name, Support::Unsupported, "No macOS driver for this controller", &[]),
        });
    }
    for d in hw.devices.iter().filter(|d| d.class == PciClass::Wireless) {
        let name = d.name.clone();
        rows.push(match d.vendor {
            0x8086 => row(
                "Wi-Fi",
                name,
                Support::Limited,
                "itlwm (use the HeliPort app to connect); no AirDrop/Handoff",
                &["itlwm"],
            ),
            0x14E4 => row(
                "Wi-Fi",
                name,
                Support::Limited,
                "Native driver; Sonoma and newer need root patches",
                &["AirportBrcmFixup"],
            ),
            _ => row("Wi-Fi", name, Support::Unsupported, "No macOS driver: replace the card or use a USB adapter", &[]),
        });
    }
    for (vendor, product) in &hw.bluetooth {
        let name = format!("{} Bluetooth [{:04x}:{:04x}]", vendor_label(*vendor), vendor, product);
        rows.push(match *vendor {
            0x8087 => row(
                "Bluetooth",
                name,
                Support::Limited,
                "IntelBluetoothFirmware + IntelBTPatcher (BlueToolFixup on macOS 12+)",
                &["IntelBluetoothFirmware", "IntelBTPatcher", "IntelBluetoothInjector", "BlueToolFixup"],
            ),
            0x0A5C => row(
                "Bluetooth",
                name,
                Support::Supported,
                "BrcmPatchRAM3 + BlueToolFixup",
                &["BlueToolFixup", "BrcmBluetoothInjector", "BrcmFirmwareData", "BrcmPatchRAM3"],
            ),
            _ => row("Bluetooth", name, Support::Unsupported, "No macOS driver for this adapter", &[]),
        });
    }
}

fn other_rows(hw: &Hardware, rows: &mut Vec<Component>) {
    if let Some(dev) = hw.devices.iter().find(|d| d.class == PciClass::Audio) {
        let name = hw.codec.clone().unwrap_or_else(|| dev.name.clone());
        rows.push(row(
            "Audio",
            name,
            Support::Supported,
            "AppleALC with layout-id 1 (alcid=1); try other layouts if there is no sound",
            &["AppleALC"],
        ));
    }
    for d in hw.devices.iter().filter(|d| d.class == PciClass::Nvme) {
        rows.push(row("Storage", d.name.clone(), Support::Supported, "NVMe: NVMeFix for power management", &["NVMeFix"]));
    }
    for d in hw.devices.iter().filter(|d| d.class == PciClass::Sata) {
        rows.push(row("Storage", d.name.clone(), Support::Supported, "SATA (AHCI): native", &[]));
    }
    for d in hw.devices.iter().filter(|d| d.class == PciClass::Raid) {
        rows.push(row(
            "Storage",
            d.name.clone(),
            Support::Unsupported,
            "Intel RST/VMD RAID mode: set the controller to AHCI in BIOS",
            &[],
        ));
    }
    if hw.ps2 {
        rows.push(row(
            "Input",
            "PS/2 keyboard / trackpad".to_string(),
            Support::Supported,
            "VoodooPS2",
            &["VoodooPS2"],
        ));
    }
    if hw.i2c {
        rows.push(row(
            "Input",
            "I2C HID trackpad / touchscreen".to_string(),
            Support::Limited,
            "VoodooI2C: GPIO pinning or polling mode may be required",
            &["VoodooI2C"],
        ));
    }
}

pub fn analyze(hw: &Hardware) -> Report {
    let mut rows = vec![cpu_row(hw)];
    rows.extend(igpu_row(hw));
    discrete_rows(hw, &mut rows);
    other_rows(hw, &mut rows);
    network_rows(hw, &mut rows);

    let (mut min, mut max) = (11, 26);
    let mut blocked = None;
    for r in &rows {
        if r.blocking && r.support == Support::Unsupported && blocked.is_none() {
            blocked = Some(format!("{}: {}", r.category, r.note));
        }
        if r.limits_range && r.support != Support::Unsupported {
            min = min.max(r.min);
            max = max.min(r.max);
        }
    }
    if blocked.is_none() && min > max {
        blocked = Some("No macOS version works with every component".to_string());
    }
    Report { rows, min, max, blocked }
}

pub fn available(report: &Report) -> Vec<&'static MacOs> {
    VERSIONS
        .iter()
        .filter(|m| m.major >= report.min && m.major <= report.max)
        .collect()
}

pub fn default_version(report: &Report) -> &'static MacOs {
    available(report).last().copied().unwrap_or(&VERSIONS[0])
}

pub fn parse_version(report: &Report, value: &str) -> Result<&'static MacOs> {
    let wanted = value.to_lowercase().replace(' ', "");
    available(report)
        .into_iter()
        .find(|m| m.name.to_lowercase().replace(' ', "") == wanted || m.major.to_string() == wanted)
        .ok_or_else(|| anyhow!("macOS version \"{value}\" is not compatible with this hardware"))
}
