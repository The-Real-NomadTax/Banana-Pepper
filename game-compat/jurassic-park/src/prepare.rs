//! Identify and snapshot the two supplied Jurassic Park builds without modification.
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub enum Build {
    Original,
    Legacy,
    Native,
}
impl Build {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Original => "game_original",
            Self::Legacy | Self::Native => "game_legacy_patched",
        }
    }
    pub fn sha256(self) -> &'static str {
        match self {
            Self::Original => "6bb8a70dc0ee3f9d7ac3672d7dc8cf4aadec44b0f96c050ace96a7633d26af11",
            Self::Legacy | Self::Native => {
                "c18a9329e72a363937155d245440694e58b5a1e27984016ae7648af3413b4269"
            }
        }
    }
}

fn desktop_patches(bytes: &mut [u8]) -> Result<(), String> {
    const INIT: usize = 0x81bab40 - 0x8048000;
    const BRANCH: usize = 0x827e11b - 0x8048000;
    if bytes.get(INIT..INIT + 8) != Some(&[0x55, 0x57, 0x56, 0x53, 0x81, 0xec, 0x3c, 0x03])
        || bytes.get(BRANCH..BRANCH + 6) != Some(&[0x0f, 0x84, 0x5f, 0x01, 0, 0])
    {
        return Err("Unsupported Jurassic desktop adapter sites; no patches applied".into());
    }
    bytes[INIT..INIT + 3].copy_from_slice(&[0x31, 0xc0, 0xc3]);
    // The launcher requires complete //\n-marked plaintext script backups and
    // binds that directory read-only before allowing this profile to execute.
    bytes[BRANCH..BRANCH + 6].fill(0x90);
    Ok(())
}

pub fn transform(mut bytes: Vec<u8>, build: Build, desktop: bool) -> Result<Vec<u8>, String> {
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if hash != build.sha256() {
        return Err(format!(
            "Unrecognized Jurassic Park build: {hash}. No changes made."
        ));
    }
    if matches!(build, Build::Native) {
        let elf = goblin::elf::Elf::parse(&bytes).map_err(|e| e.to_string())?;
        let indices: Vec<_> = elf
            .dynsyms
            .iter()
            .enumerate()
            .filter_map(|(i, s)| (s.st_shndx == 0 && s.st_value != 0).then_some(i))
            .collect();
        if indices.len() != 9 {
            return Err("Unexpected Jurassic import metadata; repair not applied".into());
        }
        for index in indices {
            bytes[0x26680 + 16 * index + 4..0x26680 + 16 * index + 8].fill(0);
        }
    }
    if desktop {
        desktop_patches(&mut bytes)?;
    }
    Ok(bytes)
}
