use crate::builder::{self, State};
use crate::compat::{self, macos_name, Support};
use crate::config;
use crate::hardware::{Hardware, PciClass};
use anyhow::Result;
use std::io::{self, Write};

fn color_on() -> bool {
    !cfg!(windows) || std::env::var("WT_SESSION").is_ok()
}

fn paint(code: &str, text: &str) -> String {
    if color_on() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn clear() {
    if cfg!(windows) {
        let _ = std::process::Command::new("cmd").args(["/C", "cls"]).status();
    } else {
        print!("\x1b[2J\x1b[H");
    }
}

/// Reads one line; at end of input it answers "q" so every screen can bail out.
fn prompt(text: &str) -> String {
    print!("{text}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => "q".to_string(),
        Ok(_) => line.trim().to_string(),
    }
}

fn pause() {
    prompt("\nPress Enter to continue...");
}

fn header(title: &str) {
    clear();
    let line = "=".repeat(72);
    println!("{}", paint("36", &line));
    println!("  {}", paint("1", title));
    println!("{}\n", paint("36", &line));
}

fn fit(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        format!("{text:<width$}")
    } else {
        let cut: String = chars[..width - 1].iter().collect();
        format!("{cut}~")
    }
}

fn label(major: u32) -> String {
    format!("{} ({})", macos_name(major), major)
}

fn range(min: u32, max: u32) -> String {
    if min == max {
        label(min)
    } else {
        format!("{} - {}", label(min), label(max))
    }
}

fn status(support: Support) -> String {
    match support {
        Support::Supported => paint("32", "Supported  "),
        Support::Limited => paint("33", "Limited    "),
        Support::Unsupported => paint("31", "Unsupported"),
    }
}

fn describe(hw: &Hardware, class: PciClass) -> Vec<String> {
    hw.devices
        .iter()
        .filter(|d| d.class == class)
        .map(|d| {
            let id = format!("{:04x}:{:04x}", d.vendor, d.device);
            if d.name == id {
                id
            } else {
                format!("{} [{}]", d.name, id)
            }
        })
        .collect()
}

fn line(name: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    for (i, value) in values.iter().enumerate() {
        println!("{:<13}{}", if i == 0 { name } else { "" }, value);
    }
}

pub fn print_report(state: &State) {
    let hw = &state.hw;
    println!("{}", paint("1", "Hardware report"));
    println!("{:<13}{}", "CPU", hw.cpu_name);
    println!("{:<13}{} (generation {})", "Codename", hw.codename(), hw.generation);
    println!("{:<13}{}", "Form factor", if hw.is_laptop { "laptop" } else { "desktop" });
    line("GPU", &describe(hw, PciClass::Display));
    let mut audio = describe(hw, PciClass::Audio);
    if let Some(codec) = &hw.codec {
        audio.insert(0, format!("codec {codec}"));
    }
    line("Audio", &audio);
    line("Ethernet", &describe(hw, PciClass::Ethernet));
    line("Wi-Fi", &describe(hw, PciClass::Wireless));
    let bluetooth: Vec<String> = hw
        .bluetooth
        .iter()
        .map(|(v, p)| format!("{} {:04x}:{:04x}", crate::hardware::vendor_label(*v), v, p))
        .collect();
    line("Bluetooth", &bluetooth);
    let mut storage = describe(hw, PciClass::Nvme);
    storage.extend(describe(hw, PciClass::Sata));
    storage.extend(describe(hw, PciClass::Raid));
    line("Storage", &storage);
    let mut input = Vec::new();
    if hw.ps2 {
        input.push("PS/2".to_string());
    }
    if hw.i2c {
        input.push("I2C HID".to_string());
    }
    line("Input", &input);
    println!();
}

pub fn print_compat(state: &State) {
    println!("{}", paint("1", "Hardware compatibility"));
    for r in &state.report.rows {
        println!(
            "{:<10} {} {}  {}",
            r.category,
            status(r.support),
            fit(&r.name, 38),
            if r.support == Support::Unsupported { String::new() } else { range(r.min, r.max) }
        );
        if !r.note.is_empty() {
            println!("{:<10} {}", "", paint("2", &r.note));
        }
    }
    println!();
    match &state.report.blocked {
        Some(reason) => println!("{} {}", paint("31", "Cannot build:"), reason),
        None => println!(
            "Compatible macOS versions: {}",
            range(state.report.min, state.report.max)
        ),
    }
}

fn show_report(state: &State) {
    header("Hardware report");
    print_report(state);
    pause();
}

fn show_compat(state: &State) {
    header("Hardware compatibility");
    print_compat(state);
    pause();
}

fn pick_macos(state: &mut State) {
    header("Select macOS version");
    let list = compat::available(&state.report);
    if list.is_empty() {
        println!("No macOS version is compatible with this hardware.");
        pause();
        return;
    }
    for (i, m) in list.iter().enumerate() {
        let mark = if m.major == state.macos.major { "*" } else { " " };
        println!("{:>2}. {} {} ({})", i + 1, mark, m.name, m.major);
    }
    let input = prompt("\nSelect a version (Enter to keep): ");
    if let Some(m) = input.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| list.get(i)).copied() {
        state.set_macos(m);
    }
}

fn edit_kexts(state: &mut State) {
    loop {
        header("Customize kexts");
        for (i, c) in state.kexts.iter().enumerate() {
            let mark = if c.enabled { paint("32", "[x]") } else { "[ ]".to_string() };
            let note = if c.reason.is_empty() {
                c.def.desc.to_string()
            } else {
                format!("{} - {}", c.def.desc, c.reason)
            };
            println!("{:>2}. {} {:<24} {}", i + 1, mark, c.def.id, fit(&note, 40));
        }
        println!("\nEnter a number to toggle a kext, Enter to go back.");
        let input = prompt("> ");
        if matches!(input.as_str(), "" | "q" | "b") {
            return;
        }
        if let Some(index) = input.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) {
            if let Some(choice) = state.kexts.get_mut(index) {
                if choice.def.id == "Lilu" {
                    println!("Lilu is required by most kexts and cannot be disabled.");
                    pause();
                } else {
                    choice.enabled = !choice.enabled;
                }
            }
        }
    }
}

fn edit_acpi(state: &mut State) {
    loop {
        header("Customize ACPI patches");
        for (i, p) in state.acpi.iter().enumerate() {
            let mark = if p.enabled { paint("32", "[x]") } else { "[ ]".to_string() };
            println!("{:>2}. {} {:<14} {}", i + 1, mark, p.name, p.desc);
        }
        println!("\nEnter a number to toggle a patch, Enter to go back.");
        let input = prompt("> ");
        if matches!(input.as_str(), "" | "q" | "b") {
            return;
        }
        if let Some(index) = input.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) {
            if let Some(patch) = state.acpi.get_mut(index) {
                patch.enabled = !patch.enabled;
            }
        }
    }
}

fn pick_smbios(state: &mut State) {
    header("Customize SMBIOS model");
    let models = config::models(&state.hw);
    for (i, (name, max)) in models.iter().enumerate() {
        let mark = if *name == state.smbios { "*" } else { " " };
        let warn = if *max >= state.macos.major { "" } else { "  (too old for the selected macOS)" };
        println!("{:>2}. {} {:<16} up to {}{}", i + 1, mark, name, label(*max), warn);
    }
    let input = prompt("\nSelect a model (Enter to keep): ");
    if let Some((name, _)) = input.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| models.get(i)) {
        state.smbios = name.to_string();
    }
}

fn build(state: &State) {
    header("Build OpenCore EFI");
    if let Some(reason) = &state.report.blocked {
        println!("{} {}", paint("31", "Cannot build:"), reason);
        pause();
        return;
    }
    println!("macOS {} ({}), SMBIOS {}\n", state.macos.name, state.macos.major, state.smbios);
    if let Err(error) = builder::run_build(state) {
        println!("\n{} {error:#}", paint("31", "Build failed:"));
    }
    pause();
}

pub fn run(state: &mut State) -> Result<()> {
    loop {
        header("OpCore Simplify");
        println!("CPU     {}", state.hw.cpu_name);
        match &state.report.blocked {
            Some(reason) => println!("Status  {} {}", paint("31", "not compatible:"), reason),
            None => println!(
                "Status  compatible with {}",
                range(state.report.min, state.report.max)
            ),
        }
        println!();
        let enabled = state.kexts.iter().filter(|c| c.enabled).count();
        let patches = state.acpi.iter().filter(|p| p.enabled).count();
        println!(" 1. Hardware report");
        println!(" 2. Hardware compatibility");
        println!(" 3. Select macOS version      {}", label(state.macos.major));
        println!(" 4. Customize ACPI patches    {} of {} enabled", patches, state.acpi.len());
        println!(" 5. Customize kexts           {} enabled", enabled);
        println!(" 6. Customize SMBIOS model    {}", state.smbios);
        if state.xe() {
            println!(" D. DVMT 60MB patch (Xe)      {}", if state.dvmt_60 { "on" } else { "off" });
        }
        println!(" 7. Build OpenCore EFI        -> {}", state.out.display());
        println!(" Q. Quit\n");

        match prompt("Select an option: ").to_lowercase().as_str() {
            "1" => show_report(state),
            "2" => show_compat(state),
            "3" => pick_macos(state),
            "4" => edit_acpi(state),
            "5" => edit_kexts(state),
            "6" => pick_smbios(state),
            "7" => build(state),
            "d" if state.xe() => state.dvmt_60 = !state.dvmt_60,
            "q" => return Ok(()),
            _ => {}
        }
    }
}
