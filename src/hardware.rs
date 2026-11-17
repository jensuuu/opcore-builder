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
    Sata,
    Raid,
    Other,
}

#[derive(Clone, Debug)]
pub struct PciDevice {
    pub vendor: u16,
    pub device: u16,
    pub class: PciClass,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Hardware {
    pub cpu_name: String,
    pub intel: bool,
    pub generation: u32,
    pub ice_lake: bool,
    pub is_laptop: bool,
    pub devices: Vec<PciDevice>,
    pub codec: Option<String>,
    pub bluetooth: Vec<(u16, u16)>,
    pub ps2: bool,
    pub i2c: bool,
}

#[derive(Default)]
struct Extras {
    codec: Option<String>,
    bluetooth: Vec<(u16, u16)>,
    ps2: bool,
    i2c: bool,
}

pub fn vendor_label(vendor: u16) -> &'static str {
    match vendor {
        0x8086 | 0x8087 => "Intel",
        0x10DE => "NVIDIA",
        0x1002 | 0x1022 => "AMD",
        0x10EC => "Realtek",
        0x14E4 | 0x0A5C => "Broadcom",
        0x168C => "Qualcomm Atheros",
        0x14C3 => "MediaTek",
        _ => "Unknown",
    }
}

impl Hardware {
    fn new(cpu_name: String, is_laptop: bool, devices: Vec<PciDevice>, extras: Extras) -> Self {
        let intel = cpu_name.contains("Intel");
        let generation = generation(&cpu_name).unwrap_or(0);
        let ice_lake = model_digits(&cpu_name)
            .map(|d| d.len() == 4 && d.starts_with("10"))
            .unwrap_or(false);
        // laptops without a detected keyboard controller still use PS/2 for the keyboard
        let ps2 = extras.ps2 || (is_laptop && !extras.i2c);
        Self {
            cpu_name,
            intel,
            generation,
            ice_lake,
            is_laptop,
            devices,
            codec: extras.codec,
            bluetooth: extras.bluetooth,
            ps2,
            i2c: extras.i2c,
        }
    }

    pub fn codename(&self) -> &'static str {
        if !self.intel {
            return "Unknown";
        }
        match self.generation {
            1..=5 => "Pre-Skylake",
            6 => "Skylake",
            7 => "Kaby Lake",
            8 => "Coffee Lake / Kaby Lake Refresh",
            9 => "Coffee Lake Refresh",
            10 if self.ice_lake => "Ice Lake",
            10 => "Comet Lake",
            11 if self.is_laptop => "Tiger Lake",
            11 => "Rocket Lake",
            12 => "Alder Lake",
            13 => "Raptor Lake",
            14 => "Raptor Lake Refresh",
            _ => "Unknown",
        }
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

fn load_ids() -> Option<String> {
    ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids", "/usr/share/pci.ids"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
}

fn pci_name(ids: &Option<String>, vendor: u16, device: u16) -> String {
    let fallback = format!("{vendor:04x}:{device:04x}");
    let Some(text) = ids else { return fallback };
    let vendor_key = format!("{vendor:04x}  ");
    let device_key = format!("\t{device:04x}  ");
    let mut in_vendor = false;
    let mut vendor_name = String::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        if !line.starts_with('\t') {
            in_vendor = line.starts_with(&vendor_key);
            if in_vendor {
                vendor_name = line[vendor_key.len()..].to_string();
            }
        } else if in_vendor && line.starts_with(&device_key) {
            return format!("{} {}", vendor_name, &line[device_key.len()..]);
        }
    }
    fallback
}

fn linux_codec() -> Option<String> {
    for card in std::fs::read_dir("/proc/asound").ok()?.flatten() {
        let path = card.path();
        if !card.file_name().to_string_lossy().starts_with("card") {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&path) else { continue };
        for file in entries.flatten() {
            if !file.file_name().to_string_lossy().starts_with("codec#") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(file.path()) else { continue };
            let Some(line) = text.lines().find(|l| l.starts_with("Codec:")) else { continue };
            let name = line.trim_start_matches("Codec:").trim().to_string();
            let lower = name.to_lowercase();
            if !lower.contains("hdmi") && !lower.contains("displayport") {
                return Some(name);
            }
        }
    }
    None
}

fn linux_bluetooth() -> Vec<(u16, u16)> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/bus/usb/devices") else { return found };
    for entry in entries.flatten() {
        let path = entry.path();
        let read = |file: &str| std::fs::read_to_string(path.join(file)).ok().map(|s| s.trim().to_string());
        if read("bDeviceClass").as_deref() != Some("e0") {
            continue;
        }
        let (Some(vendor), Some(product)) = (read("idVendor"), read("idProduct")) else { continue };
        if let (Ok(v), Ok(p)) = (u16::from_str_radix(&vendor, 16), u16::from_str_radix(&product, 16)) {
            found.push((v, p));
        }
    }
    found
}

fn linux_input() -> (bool, bool) {
    let text = std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default();
    let (mut ps2, mut i2c) = (false, false);
    for line in text.lines().filter(|l| l.starts_with("N: Name=")) {
        let name = line.to_lowercase();
        if name.contains("at translated") || name.contains("ps/2") {
            ps2 = true;
        }
        if name.contains("i2c") || (name.contains(":00 ") && name.contains("touchpad")) {
            i2c = true;
        }
    }
    (ps2, i2c)
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

    let ids = load_ids();
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
        } else if class.starts_with("0106") {
            PciClass::Sata
        } else if class.starts_with("0104") && vendor == 0x8086 {
            PciClass::Raid
        } else {
            PciClass::Other
        };
        let name = pci_name(&ids, vendor, device);
        devices.push(PciDevice { vendor, device, class: kind, name });
    }
    let (ps2, i2c) = linux_input();
    let extras = Extras { codec: linux_codec(), bluetooth: linux_bluetooth(), ps2, i2c };
    Ok(Hardware::new(cpu_name, is_laptop, devices, extras))
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
        let friendly = item["FriendlyName"].as_str().unwrap_or_default().to_string();
        let name = friendly.to_lowercase();
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
            _ if name.contains("sata") || name.contains("ahci") => PciClass::Sata,
            _ if name.contains("raid") && vendor == 0x8086 => PciClass::Raid,
            _ => PciClass::Other,
        };
        let name = if friendly.is_empty() { format!("{vendor:04x}:{device:04x}") } else { friendly };
        devices.push(PciDevice { vendor, device, class: kind, name });
    }
    Ok(Hardware::new(cpu_name, is_laptop, devices, windows_extras()))
}

fn windows_extras() -> Extras {
    let raw = powershell("Get-PnpDevice -PresentOnly -Class Bluetooth | ForEach-Object { $_.InstanceId }").unwrap_or_default();
    let mut bluetooth = Vec::new();
    for line in raw.lines() {
        let id = line.trim().to_uppercase();
        if id.starts_with("USB\\VID_") {
            if let (Some(vendor), Some(product)) = (hex_after(&id, "VID_"), hex_after(&id, "PID_")) {
                bluetooth.push((vendor, product));
            }
        }
    }
    let i2c = powershell("@(Get-PnpDevice -PresentOnly | Where-Object { $_.FriendlyName -like '*I2C HID*' }).Count")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0)
        > 0;
    Extras { codec: None, bluetooth, ps2: false, i2c }
}
