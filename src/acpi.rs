use crate::hardware::Hardware;
use anyhow::Result;
use std::path::Path;

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

pub fn install(samples: &Path, destination: &Path, hw: &Hardware) -> Result<Vec<String>> {
    std::fs::create_dir_all(destination)?;
    let mut groups: Vec<Vec<&str>> = vec![vec!["SSDT-PLUG.aml"]];
    groups.push(if hw.is_laptop {
        vec!["SSDT-EC-USBX-LAPTOP.aml", "SSDT-EC-USBX.aml"]
    } else {
        vec!["SSDT-EC-USBX-DESKTOP.aml", "SSDT-EC-USBX.aml"]
    });
    if hw.generation >= 9 {
        groups.push(vec!["SSDT-AWAC.aml"]);
    }

    let mut installed = Vec::new();
    for group in groups {
        if let Some(name) = group.into_iter().find(|n| samples.join(n).exists()) {
            std::fs::copy(samples.join(name), destination.join(name))?;
            installed.push(name.to_string());
        }
    }
    if hw.is_laptop && hw.igpu().is_some() {
        std::fs::write(destination.join("SSDT-PNLF.aml"), pnlf())?;
        installed.push("SSDT-PNLF.aml".to_string());
    }
    Ok(installed)
}
