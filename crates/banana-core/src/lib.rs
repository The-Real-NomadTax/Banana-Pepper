//! Host-independent discovery, executable identification and library
//! state. No game execution, host graphics APIs or individual games belong
//! in this crate: games are reached through the `banana-api` registry.
pub use banana_api::{BinaryInfo, Controls};
use banana_api::GameCompat;
use goblin::Object;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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

impl GameProfile {
    fn of(game: &dyn GameCompat) -> Self {
        Self {
            id: game.id().into(),
            title: game.title().into(),
            executable: game.default_executable().into(),
            controls: game.controls(),
        }
    }
}

/// Paths are relative to the selected games root, never to a developer's home.
pub fn profiles() -> Vec<GameProfile> {
    banana_api::games().iter().map(|g| GameProfile::of(*g)).collect()
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
    banana_api::games().iter().map(|game| report(*game, root)).collect()
}

/// Inspect one game's executable below `root`.
pub fn report(game: &dyn GameCompat, root: &Path) -> GameReport {
    let path = game.locate_executable(root);
    let result = inspect_file(&path);
    let blockers = match game.compatibility_notes() {
        Some(notes) => notes.iter().map(|n| (*n).to_owned()).collect(),
        None => generic_notes(result.as_ref().ok()),
    };
    let (binary, error) = match result {
        Ok(b) => (Some(b), None),
        Err(e) => (None, Some(e)),
    };
    GameReport { profile: GameProfile::of(game), path, binary, error, blockers }
}

/// Notes for a game that has not described its own compatibility state.
fn generic_notes(info: Option<&BinaryInfo>) -> Vec<String> {
    let mut blockers = vec![
        "Cabinet I/O protocol and game adapter are not implemented.".into(),
        "Required security hardware/services have not been validated.".into(),
    ];
    if let Some(info) = info {
        if info.format == "ELF" {
            blockers.push(format!(
                "Linux {}-bit userspace, library ABI and cabinet filesystem need validation.",
                info.bits
            ));
            if info.section_count == 0 {
                blockers.push("Section table absent; inspection uses ELF program/dynamic headers.".into());
            }
        } else {
            blockers.push("Windows execution backend is not implemented.".into());
        }
    }
    blockers
}

/// SHA-256 of a file, lowercase hex.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// The recognised build of `game` at `path`, if any.
pub fn identify_build(game: &dyn GameCompat, path: &Path) -> Option<&'static banana_api::KnownBuild> {
    let hash = sha256_file(path).ok()?;
    game.known_builds().iter().find(|b| b.sha256 == hash)
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
    fn aim_handles_invalid_and_out_of_range_values() {
        let mut input = CabinetInput::default();
        input.set_aim(-1.0, 2.0);
        assert_eq!(input.aim, [0.0, 1.0]);
        input.set_aim(f32::NAN, f32::INFINITY);
        assert_eq!(input.aim, [0.5, 0.5]);
    }
}
