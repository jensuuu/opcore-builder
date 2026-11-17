use crate::hardware::Hardware;
use anyhow::Result;
use std::path::Path;

pub struct Patch {
    pub name: &'static str,
    pub desc: &'static str,
    pub enabled: bool,
    candidates: Vec<&'static str>,
    generated: bool,
}

pub fn patches(hw: &Hardware) -> Vec<Patch> {
    let mut list = vec![
        Patch {
            name: "SSDT-PLUG",
            desc: "CPU power management (plugin-type)",
            enabled: true,
            candidates: vec!["SSDT-PLUG.aml"],
            generated: false,
        },
        Patch {
            name: "SSDT-EC-USBX",
            desc: "Fake embedded controller + USB power properties",
            enabled: true,
            candidates: if hw.is_laptop {
                vec!["SSDT-EC-USBX-LAPTOP.aml", "SSDT-EC-USBX.aml"]
            } else {
                vec!["SSDT-EC-USBX-DESKTOP.aml", "SSDT-EC-USBX.aml"]
            },
            generated: false,
        },
        Patch {
            name: "SSDT-AWAC",
            desc: "Enable the legacy RTC clock (300-series boards and newer)",
            enabled: hw.generation >= 9,
            candidates: vec!["SSDT-AWAC.aml"],
            generated: false,
        },
    ];
    if hw.is_laptop && hw.igpu().is_some() {
        list.push(Patch {
            name: "SSDT-PNLF",
            desc: "Backlight control for the internal display",
            enabled: true,
            candidates: vec![],
            generated: true,
        });
    }
    list
}

fn name_op(name: &str, value: Vec<u8>) -> Vec<u8> {
    let mut bytes = vec![0x08];
    bytes.extend(name.as_bytes());
    bytes.extend(value);
    bytes
}

fn package(op: &[u8], name: &[u8], body: &[u8]) -> Vec<u8> {
    let length = (body.len() + name.len() + 1) as u8;
    let mut bytes = op.to_vec();
    bytes.push(length);
    bytes.extend(name);
    bytes.extend(body);
    bytes
}

fn pnlf() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(name_op("_HID", vec![0x0C, 0x06, 0x10, 0x00, 0x02]));
    let mut cid = vec![0x0D];
    cid.extend(b"backlight");
    cid.push(0);
    body.extend(name_op("_CID", cid));
    body.extend(name_op("_UID", vec![0x0A, 0x0A]));
    body.extend(name_op("_STA", vec![0x0A, 0x0B]));
    let device = package(&[0x5B, 0x82], b"PNLF", &body);
    let scope = package(&[0x10], b"\\_SB_", &device);

    let total = 36 + scope.len();
    let mut table = Vec::with_capacity(total);
    table.extend(b"SSDT");
    table.extend((total as u32).to_le_bytes());
    table.push(2);
    table.push(0);
    table.extend(b"APPLE ");
    table.extend(b"PNLF    ");
    table.extend(0u32.to_le_bytes());
    table.extend(b"INTL");
    table.extend(0x20200925u32.to_le_bytes());
    table.extend(&scope);
    let sum = table.iter().fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    table[9] = 0u8.wrapping_sub(sum);
    table
}

pub fn install(samples: &Path, destination: &Path, patches: &[Patch]) -> Result<Vec<String>> {
    std::fs::create_dir_all(destination)?;
    let mut installed = Vec::new();
    for patch in patches.iter().filter(|p| p.enabled) {
        if patch.generated {
            let file = format!("{}.aml", patch.name);
            std::fs::write(destination.join(&file), pnlf())?;
            installed.push(file);
        } else if let Some(name) = patch.candidates.iter().find(|n| samples.join(n).exists()) {
            std::fs::copy(samples.join(name), destination.join(name))?;
            installed.push(name.to_string());
        } else {
            eprintln!("skipped {}: sample file not found in the OpenCore package", patch.name);
        }
    }
    Ok(installed)
}
