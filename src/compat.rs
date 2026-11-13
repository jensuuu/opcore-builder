use crate::hardware::Hardware;
use anyhow::{anyhow, bail, Result};
use std::io::Write;

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

pub fn check(hw: &Hardware) -> Result<()> {
    if !hw.intel {
        bail!("Only Intel processors are supported");
    }
    if !(6..=11).contains(&hw.generation) {
        bail!("Unsupported processor generation: {}", hw.generation);
    }
    if hw.rocket_lake_igpu() && hw.discrete().is_empty() {
        bail!("Rocket Lake graphics are unsupported and no discrete GPU was found");
    }
    Ok(())
}

pub fn compatible(hw: &Hardware) -> Vec<&'static MacOs> {
    VERSIONS
        .iter()
        .filter(|m| !(hw.ice_lake && hw.discrete().is_empty() && m.major > 14))
        .collect()
}

pub fn choose(hw: &Hardware, requested: Option<String>) -> Result<&'static MacOs> {
    let list = compatible(hw);
    if let Some(value) = requested {
        let wanted = value.to_lowercase().replace(' ', "");
        return list
            .iter()
            .copied()
            .find(|m| m.name.to_lowercase().replace(' ', "") == wanted || m.major.to_string() == wanted)
            .ok_or_else(|| anyhow!("macOS version is not compatible with this hardware"));
    }
    for (index, version) in list.iter().enumerate() {
        println!("{}. {} ({})", index + 1, version.name, version.major);
    }
    print!("Select macOS version [{}]: ", list.len());
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let index = line
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|i| (1..=list.len()).contains(i))
        .unwrap_or(list.len());
    Ok(list[index - 1])
}
