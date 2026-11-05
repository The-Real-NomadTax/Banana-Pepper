//! Host-independent discovery, static analysis and cabinet state.
//! No game execution or host graphics APIs belong in this crate.
pub mod aliens;
pub mod big_buck;
pub mod controls;
pub mod jurassic;
pub mod minecraft;
pub mod settings;

use goblin::Object;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

const MAX_EXECUTABLE_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameProfile {
    pub id: String,
    pub title: String,
    pub executable: PathBuf,
    pub controls: Controls,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Controls {
    LightGuns,
    Driving,
    Joysticks,
}

/// Paths are relative to the selected games root, never to a developer's home.
pub fn profiles() -> Vec<GameProfile> {
    [
        ("aliens", "Aliens Armageddon", "linux based/Aliens Armageddon/game", Controls::LightGuns),
        ("jurassic", "Jurassic Park 2015", "linux based/Jurassic Park 2015/game_original", Controls::LightGuns),
        ("big-buck", "Big Buck Hunter Pro", "linux based/Big Bug Hunter Pro/game", Controls::LightGuns),
        ("super-cars", "The Fast and the Furious Super Cars", "windows based/The_Fast_and_the_Furious_SuperCars/The_Fast_and_the_Furious_SuperCars/sdaemon.exe", Controls::Driving),
        ("minecraft", "Minecraft Dungeons Arcade", "linux based/Minecraft Dungeons Arcade/gamedump.exec", Controls::Joysticks),
    ].into_iter().map(|(id,title,path,controls)| GameProfile {
        id: id.into(), title: title.into(), executable: path.into(), controls,
    }).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryInfo {
    pub format: String,
    pub bits: u8,
    pub machine: u16,
    pub entry: u64,
    pub interpreter: Option<String>,
    pub libraries: Vec<String>,
    pub search_paths: Vec<String>,
    pub section_count: usize,
}

pub fn inspect_bytes(bytes: &[u8]) -> Result<BinaryInfo, String> {
    match Object::parse(bytes).map_err(|e| e.to_string())? {
        Object::Elf(elf) => Ok(BinaryInfo {
            format: "ELF".into(),
            bits: if elf.is_64 { 64 } else { 32 },
            machine: elf.header.e_machine,
            entry: elf.entry,
            interpreter: elf.interpreter.map(str::to_owned),
            libraries: elf.libraries.iter().map(|s| s.to_string()).collect(),
            search_paths: elf
                .rpaths
                .iter()
                .chain(elf.runpaths.iter())
                .map(|s| s.to_string())
                .collect(),
            section_count: elf.section_headers.len(),
        }),
        Object::PE(pe) => Ok(BinaryInfo {
            format: "PE".into(),
            bits: if pe.is_64 { 64 } else { 32 },
            machine: pe.header.coff_header.machine,
            entry: pe.image_base + pe.entry as u64,
            interpreter: None,
            libraries: pe.libraries.iter().map(|s| s.to_string()).collect(),
            search_paths: vec![],
            section_count: pe.sections.len(),
        }),
        _ => Err("Unsupported executable format; expected ELF or PE".into()),
    }
}

pub fn inspect_file(path: &Path) -> Result<BinaryInfo, String> {
    // Bound allocation even if the file changes between metadata and reading.
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_EXECUTABLE_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_EXECUTABLE_SIZE {
        return Err("Executable exceeds 256 MiB inspection limit".into());
    }
    inspect_bytes(&bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameReport {
    pub profile: GameProfile,
    pub path: PathBuf,
    pub binary: Option<BinaryInfo>,
    pub error: Option<String>,
    pub blockers: Vec<String>,
}

pub fn scan(root: &Path) -> Vec<GameReport> {
    profiles().into_iter().map(|profile| {
        let path = if profile.id == "minecraft" { minecraft::executable(root) } else { root.join(&profile.executable) };
        let result = inspect_file(&path);
        let mut blockers = vec!["Cabinet I/O protocol and game adapter are not implemented.".into(),
            "Required security hardware/services have not been validated.".into()];
        if let Ok(info) = &result {
            if info.format == "ELF" {
                blockers.push(format!("Linux {}-bit userspace, library ABI and cabinet filesystem need validation.", info.bits));
                if info.section_count == 0 {
                    blockers.push("Section table absent; inspection uses ELF program/dynamic headers.".into());
                }
            } else {
                blockers.push("Windows execution backend is not implemented.".into());
            }
        }
        if profile.id == "super-cars" {
            blockers = vec![
                "Runs under Wine (WineD3D) for one SHA256-verified sdaemon.exe; keyboard drives the cabinet inputs through the arcade-wine-hook dinput8.dll.".into(),
                "Steering is digital (ramped); analog wheels/gamepads, force feedback, LED display and the H2O board are not emulated.".into(),
                "The game logs DONGLE MISSING periodically; races still run, but long sessions and audits are unverified.".into(),
            ];
        }
        if profile.id == "big-buck" {
            blockers = vec![
                "Experimental Linux startup backend available for one SHA256-verified build.".into(),
                "Virtual JAMMA API supports switch edges and mouse gun callbacks; physical serial protocol is not implemented.".into(),
                "OSS playback is bridged to PulseAudio/PipeWire; full scored gameplay remains unverified.".into(),
            ];
        }
        if profile.id == "jurassic" {
            blockers = vec![
                "Native original build exits before engine startup while connecting to a Sentinel/HASP service on localhost:1947.".into(),
                "Supplied modified build crashes during native initialization.".into(),
                "Legacy 32-bit Theora and PhysX 2.8.1 libraries are staged locally; graphics and cabinet I/O are not yet validated.".into(),
            ];
        }
        if profile.id == "minecraft" {
            blockers = vec![
                "Windows uses WSL2 with WSLg; Linux uses the same isolated runner directly.".into(),
                "Run startup diagnostics to check GLU, LAPACK, wireless/PCSC and FMOD 1.10 libraries.".into(),
                "The measured startup failure is HASP status 7 (protection key not found). Startup diagnostics recheck the actual API status.".into(),
                "Cabinet joystick, card-reader and security interfaces are not implemented; gameplay is unverified.".into(),
                "Supplied gamedump.exec is a pre-existing modified build; original game and cabinet boot scripts are never run.".into(),
            ];
        }
        let (binary,error) = match result { Ok(b) => (Some(b),None), Err(e) => (None,Some(e)) };
        GameReport { profile, path, binary, error, blockers }
    }).collect()
}

pub fn write_report(path: &Path, reports: &[GameReport]) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(reports).map_err(io::Error::other)?;
    fs::write(path, bytes)
}

/// Internal state only. A game adapter must translate this into a measured
/// cabinet protocol; these fields do not claim to emulate any particular board.
#[derive(Debug, Default, Clone, Copy)]
pub struct CabinetInput {
    pub aim: [f32; 2],
    pub trigger: bool,
    pub start: bool,
    pub coin: bool,
}
impl CabinetInput {
    pub fn set_aim(&mut self, x: f32, y: f32) {
        self.aim = [x, y].map(|v| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.5
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sectionless_elf32_is_inspected() {
        // Valid ELF32 header with no section table or program headers.
        let mut bytes = vec![0u8; 52];
        bytes[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&3u16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&0x8048000u32.to_le_bytes());
        bytes[40..42].copy_from_slice(&52u16.to_le_bytes());
        bytes[42..44].copy_from_slice(&32u16.to_le_bytes());
        bytes[46..48].copy_from_slice(&40u16.to_le_bytes());
        let info = inspect_bytes(&bytes).unwrap();
        assert_eq!(info.format, "ELF");
        assert_eq!(info.bits, 32);
        assert_eq!(info.machine, 3);
        assert_eq!(info.section_count, 0);
        assert_eq!(info.entry, 0x8048000);
    }

    #[test]
    fn malformed_binaries_return_errors() {
        for bytes in [&b""[..], &b"\x7fELF"[..], &b"MZ"[..], &b"not a binary"[..]] {
            assert!(inspect_bytes(bytes).is_err());
        }
    }
    #[test]
    fn missing_games_are_reported_individually() {
        let reports = scan(Path::new("/nonexistent-arcade-test-root"));
        assert_eq!(reports.len(), 5);
        assert!(
            reports
                .iter()
                .all(|r| r.binary.is_none() && r.error.is_some())
        );
    }
    #[test]
    fn aim_handles_invalid_and_out_of_range_values() {
        let mut input = CabinetInput::default();
        input.set_aim(-1.0, 2.0);
        assert_eq!(input.aim, [0.0, 1.0]);
        input.set_aim(f32::NAN, f32::INFINITY);
        assert_eq!(input.aim, [0.5, 0.5]);
    }
}
