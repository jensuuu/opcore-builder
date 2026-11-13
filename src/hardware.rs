use anyhow::Result;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PciClass {
    Display,
    Ethernet,
    Wireless,
    Audio,
    Nvme,
    Other,
}

#[derive(Clone, Debug)]
pub struct PciDevice {
    pub vendor: u16,
    pub device: u16,
    pub class: PciClass,
}

#[derive(Clone, Debug)]
pub struct Hardware {
    pub cpu_name: String,
    pub intel: bool,
    pub generation: u32,
    pub ice_lake: bool,
    pub is_laptop: bool,
    pub devices: Vec<PciDevice>,
}

impl Hardware {
    fn new(cpu_name: String, is_laptop: bool, devices: Vec<PciDevice>) -> Self {
        let intel = cpu_name.contains("Intel");
        let generation = generation(&cpu_name).unwrap_or(0);
        let ice_lake = model_digits(&cpu_name)
            .map(|d| d.len() == 4 && d.starts_with("10"))
            .unwrap_or(false);
        Self { cpu_name, intel, generation, ice_lake, is_laptop, devices }
    }

    pub fn has(&self, class: PciClass) -> bool {
        self.devices.iter().any(|d| d.class == class)
    }

    pub fn igpu(&self) -> Option<&PciDevice> {
        self.devices
            .iter()
            .find(|d| d.class == PciClass::Display && d.vendor == 0x8086)
    }

    pub fn discrete(&self) -> Vec<&PciDevice> {
        self.devices
            .iter()
            .filter(|d| d.class == PciClass::Display && d.vendor != 0x8086)
            .collect()
    }

    pub fn nvidia_present(&self) -> bool {
        self.discrete().iter().any(|d| d.vendor == 0x10DE)
    }

    pub fn xe_igpu(&self) -> Option<&PciDevice> {
        self.igpu().filter(|d| (0x9A40..=0x9A7F).contains(&d.device))
    }

    pub fn rocket_lake_igpu(&self) -> bool {
        self.igpu().map(|d| (0x4C80..=0x4C9F).contains(&d.device)).unwrap_or(false)
    }
}

fn model_digits(name: &str) -> Option<String> {
    ["i3-", "i5-", "i7-", "i9-"].iter().find_map(|marker| {
        name.find(marker)
            .map(|p| name[p + 3..].chars().take_while(|c| c.is_ascii_digit()).collect())
    })
}

fn generation(name: &str) -> Option<u32> {
    if let Some(pos) = name.find("th Gen") {
        let digits: String = name[..pos]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if let Ok(value) = digits.parse() {
            return Some(value);
        }
    }
    let digits = model_digits(name)?;
    match digits.len() {
        5 => digits[..2].parse().ok(),
        4 if digits.starts_with('1') => digits[..2].parse().ok(),
        4 => digits[..1].parse().ok(),
        3 => Some(1),
        _ => None,
    }
}

fn is_laptop_chassis(value: u32) -> bool {
    matches!(value, 8 | 9 | 10 | 11 | 14 | 30 | 31 | 32)
}

pub fn detect() -> Result<Hardware> {
    if cfg!(windows) {
        detect_windows()
    } else {
        detect_linux()
    }
}

fn has_wireless(path: &Path) -> bool {
    std::fs::read_dir(path.join("net"))
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| e.path().join("wireless").exists())
        })
        .unwrap_or(false)
}

fn detect_linux() -> Result<Hardware> {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo")?;
    let cpu_name = cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split(':').nth(1))
        .unwrap_or("")
        .trim()
        .to_string();
    let is_laptop = std::fs::read_to_string("/sys/class/dmi/id/chassis_type")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(is_laptop_chassis)
        .unwrap_or(false);

    let mut devices = Vec::new();
    for entry in std::fs::read_dir("/sys/bus/pci/devices")? {
        let path = entry?.path();
        let read = |file: &str| {
            std::fs::read_to_string(path.join(file))
                .ok()
                .map(|s| s.trim().trim_start_matches("0x").to_string())
        };
        let (Some(vendor), Some(device), Some(class)) = (read("vendor"), read("device"), read("class")) else {
            continue;
        };
        let (Ok(vendor), Ok(device)) = (
            u16::from_str_radix(&vendor, 16),
            u16::from_str_radix(&device, 16),
        ) else {
            continue;
        };
        let wireless = class.starts_with("0280") || has_wireless(&path);
        let kind = if class.starts_with("03") {
            PciClass::Display
        } else if wireless && class.starts_with("02") {
            PciClass::Wireless
        } else if class.starts_with("0200") {
            PciClass::Ethernet
        } else if class.starts_with("04") && vendor == 0x8086 {
            PciClass::Audio
        } else if class.starts_with("0108") {
            PciClass::Nvme
        } else {
            PciClass::Other
        };
        devices.push(PciDevice { vendor, device, class: kind });
    }
    Ok(Hardware::new(cpu_name, is_laptop, devices))
}

fn powershell(script: &str) -> Result<String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn hex_after(text: &str, marker: &str) -> Option<u16> {
    let start = text.find(marker)? + marker.len();
    u16::from_str_radix(text.get(start..start + 4)?, 16).ok()
}

fn detect_windows() -> Result<Hardware> {
    let cpu_name = powershell("(Get-CimInstance Win32_Processor | Select-Object -First 1).Name")?;
    let chassis = powershell(
        "(Get-CimInstance Win32_SystemEnclosure | Select-Object -First 1).ChassisTypes -join ','",
    )?;
    let is_laptop = chassis
        .split(',')
        .filter_map(|s| s.trim().parse::<u32>().ok())
        .any(is_laptop_chassis);

    let raw = powershell(
        "ConvertTo-Json -InputObject @(Get-PnpDevice -PresentOnly | Where-Object { $_.InstanceId -like 'PCI\\VEN_*' } | Select-Object InstanceId,Class,FriendlyName) -Compress",
    )?;
    let parsed: serde_json::Value = serde_json::from_str(&raw)?;
    let items = parsed.as_array().cloned().unwrap_or_default();

    let mut devices = Vec::new();
    for item in items {
        let id = item["InstanceId"].as_str().unwrap_or_default().to_uppercase();
        let class = item["Class"].as_str().unwrap_or_default().to_lowercase();
        let name = item["FriendlyName"].as_str().unwrap_or_default().to_lowercase();
        let (Some(vendor), Some(device)) = (hex_after(&id, "VEN_"), hex_after(&id, "DEV_")) else {
            continue;
        };
        let kind = match class.as_str() {
            "display" => PciClass::Display,
            "net" if ["wi-fi", "wifi", "wireless", "802.11", "wlan"].iter().any(|k| name.contains(k)) => {
                PciClass::Wireless
            }
            "net" => PciClass::Ethernet,
            "media" if vendor == 0x8086 => PciClass::Audio,
            _ if name.contains("nvm express") => PciClass::Nvme,
            _ => PciClass::Other,
        };
        devices.push(PciDevice { vendor, device, class: kind });
    }
    Ok(Hardware::new(cpu_name, is_laptop, devices))
}
