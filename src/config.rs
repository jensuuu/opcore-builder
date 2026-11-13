use crate::compat::MacOs;
use crate::hardware::{Hardware, PciClass};
use crate::kexts::KextEntry;
use anyhow::{anyhow, Result};
use plist::{Dictionary, Value};
use rand::Rng;
use std::path::Path;

const NVRAM_GUID: &str = "7C436110-AB2A-4BBB-A880-FE41995C9F82";
const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ0123456789";
const MODELS: [(&str, &str); 4] = [
    ("iMac17,1", "MacBookPro13,1"),
    ("iMac18,3", "MacBookPro14,1"),
    ("iMac19,1", "MacBookPro15,2"),
    ("iMac20,1", "MacBookPro16,1"),
];

pub struct Options {
    pub xe: bool,
    pub dvmt_60: bool,
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

fn flag(value: bool) -> Value {
    Value::Boolean(value)
}

fn data(value: &[u8]) -> Value {
    Value::Data(value.to_vec())
}

fn int(value: i64) -> Value {
    Value::Integer(value.into())
}

fn entry(pairs: Vec<(&str, Value)>) -> Value {
    let mut dictionary = Dictionary::new();
    for (key, value) in pairs {
        dictionary.insert(key.to_string(), value);
    }
    Value::Dictionary(dictionary)
}

fn put(dictionary: &mut Dictionary, pairs: Vec<(&str, Value)>) {
    for (key, value) in pairs {
        dictionary.insert(key.to_string(), value);
    }
}

fn section<'a>(root: &'a mut Value, path: &[&str]) -> Result<&'a mut Dictionary> {
    let mut current = root;
    for key in path {
        current = current
            .as_dictionary_mut()
            .and_then(|d| d.get_mut(*key))
            .ok_or_else(|| anyhow!("missing section {key}"))?;
    }
    current
        .as_dictionary_mut()
        .ok_or_else(|| anyhow!("section is not a dictionary"))
}

fn random_string(length: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

fn uuid() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn product(hw: &Hardware, macos: &MacOs) -> &'static str {
    let by_generation: usize = match hw.generation {
        6 => 0,
        7 => 1,
        8 | 9 => 2,
        _ => 3,
    };
    let by_os: usize = if macos.major >= 26 {
        3
    } else if macos.major >= 14 {
        2
    } else if macos.major >= 13 {
        1
    } else {
        0
    };
    let model = MODELS[by_generation.max(by_os)];
    if hw.is_laptop {
        model.1
    } else {
        model.0
    }
}

fn platform_id(hw: &Hardware) -> Option<[u8; 4]> {
    hw.igpu()?;
    match (hw.generation, hw.is_laptop) {
        (6, true) => Some([0x00, 0x00, 0x16, 0x19]),
        (6, false) => Some([0x00, 0x00, 0x12, 0x19]),
        (7, true) => Some([0x00, 0x00, 0x16, 0x59]),
        (7, false) => Some([0x03, 0x00, 0x12, 0x59]),
        (10, true) if hw.ice_lake => Some([0x00, 0x00, 0x52, 0x8A]),
        (8..=10, true) => Some([0x00, 0x00, 0xA5, 0x3E]),
        (8..=10, false) => Some([0x07, 0x00, 0x9B, 0x3E]),
        _ => None,
    }
}

pub fn build(
    sample: &Path,
    hw: &Hardware,
    macos: &MacOs,
    options: &Options,
    kexts: &[KextEntry],
    acpi: &[String],
    drivers: &[String],
) -> Result<Value> {
    let mut root = Value::from_file(sample)?;

    let acpi_add: Vec<Value> = acpi
        .iter()
        .map(|name| {
            entry(vec![
                ("Comment", text(name)),
                ("Enabled", flag(true)),
                ("Path", text(name)),
            ])
        })
        .collect();
    section(&mut root, &["ACPI"])?.insert("Add".to_string(), Value::Array(acpi_add));

    put(
        section(&mut root, &["Booter", "Quirks"])?,
        vec![
            ("AvoidRuntimeDefrag", flag(true)),
            ("DevirtualiseMmio", flag(hw.generation == 11)),
            ("EnableSafeModeSlide", flag(true)),
            ("EnableWriteUnprotector", flag(false)),
            ("ProtectUefiServices", flag(!hw.is_laptop)),
            ("ProvideCustomSlide", flag(true)),
            ("RebuildAppleMemoryMap", flag(true)),
            ("SetupVirtualMap", flag(true)),
            ("SyncRuntimePermissions", flag(true)),
        ],
    );

    let mut igpu_properties = Dictionary::new();
    if options.xe {
        if options.dvmt_60 {
            igpu_properties.insert("framebuffer-patch-enable".to_string(), data(&[0x01, 0x00, 0x00, 0x00]));
            igpu_properties.insert("framebuffer-stolenmem".to_string(), data(&[0x00, 0x00, 0xC0, 0x03]));
        }
    } else if let Some(id) = platform_id(hw) {
        igpu_properties.insert("AAPL,ig-platform-id".to_string(), data(&id));
    }
    let mut properties = Dictionary::new();
    if !igpu_properties.is_empty() {
        properties.insert("PciRoot(0x0)/Pci(0x2,0x0)".to_string(), Value::Dictionary(igpu_properties));
    }
    section(&mut root, &["DeviceProperties"])?.insert("Add".to_string(), Value::Dictionary(properties));

    let kernel_add: Vec<Value> = kexts
        .iter()
        .map(|k| {
            entry(vec![
                ("Arch", text("Any")),
                ("BundlePath", text(&k.bundle)),
                ("Comment", text(&k.bundle)),
                ("Enabled", flag(true)),
                ("ExecutablePath", text(&k.exec)),
                ("MaxKernel", text("")),
                ("MinKernel", text("")),
                ("PlistPath", text(&k.plist)),
            ])
        })
        .collect();
    section(&mut root, &["Kernel"])?.insert("Add".to_string(), Value::Array(kernel_add));

    if hw.generation == 11 {
        let mut cpuid = [0u8; 16];
        cpuid[..4].copy_from_slice(&[0x55, 0x06, 0x0A, 0x00]);
        let mut mask = [0u8; 16];
        mask[..4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        put(
            section(&mut root, &["Kernel", "Emulate"])?,
            vec![("Cpuid1Data", data(&cpuid)), ("Cpuid1Mask", data(&mask))],
        );
    }

    put(
        section(&mut root, &["Kernel", "Quirks"])?,
        vec![
            ("AppleCpuPmCfgLock", flag(false)),
            ("AppleXcpmCfgLock", flag(true)),
            ("DisableIoMapper", flag(true)),
            ("DisableLinkeditJettison", flag(true)),
            ("LapicKernelPanic", flag(false)),
            ("PanicNoKextDump", flag(true)),
            ("PowerTimeoutKernelPanic", flag(true)),
            ("XhciPortLimit", flag(false)),
        ],
    );

    put(
        section(&mut root, &["Misc", "Security"])?,
        vec![
            ("ScanPolicy", int(0)),
            ("SecureBootModel", text("Disabled")),
            ("Vault", text("Optional")),
        ],
    );

    let mut boot_args = vec!["-v".to_string(), "keepsyms=1".to_string(), "debug=0x100".to_string()];
    if options.xe {
        boot_args.push("-allow3d".to_string());
        boot_args.push("-disablegfxfirmware".to_string());
        if options.dvmt_60 {
            boot_args.push("-igfxdvmt".to_string());
        }
    } else if hw.nvidia_present() {
        boot_args.push("-wegnoegpu".to_string());
    }
    if hw.has(PciClass::Audio) {
        boot_args.push("alcid=1".to_string());
    }
    let csr: [u8; 4] = if options.xe { [0x03, 0x08, 0x00, 0x00] } else { [0x00; 4] };
    put(
        section(&mut root, &["NVRAM", "Add", NVRAM_GUID])?,
        vec![
            ("boot-args", text(&boot_args.join(" "))),
            ("csr-active-config", data(&csr)),
            ("run-efi-updater", text("No")),
        ],
    );

    put(
        section(&mut root, &["PlatformInfo"])?,
        vec![
            ("Automatic", flag(true)),
            ("UpdateDataHub", flag(true)),
            ("UpdateNVRAM", flag(true)),
            ("UpdateSMBIOS", flag(true)),
            ("UpdateSMBIOSMode", text("Create")),
        ],
    );
    let rom: [u8; 6] = rand::random();
    put(
        section(&mut root, &["PlatformInfo", "Generic"])?,
        vec![
            ("MLB", text(&random_string(17))),
            ("ROM", data(&rom)),
            ("SpoofVendor", flag(true)),
            ("SystemProductName", text(product(hw, macos))),
            ("SystemSerialNumber", text(&random_string(12))),
            ("SystemUUID", text(&uuid())),
        ],
    );

    let driver_entries: Vec<Value> = drivers
        .iter()
        .map(|name| {
            entry(vec![
                ("Arguments", text("")),
                ("Comment", text(name)),
                ("Enabled", flag(true)),
                ("LoadEarly", flag(false)),
                ("Path", text(name)),
            ])
        })
        .collect();
    section(&mut root, &["UEFI"])?.insert("Drivers".to_string(), Value::Array(driver_entries));
    put(
        section(&mut root, &["UEFI", "Quirks"])?,
        vec![("ReleaseUsbOwnership", flag(true))],
    );

    Ok(root)
}
