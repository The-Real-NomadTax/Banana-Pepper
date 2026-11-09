//! The in-memory `wmn6r.exe` the session runs.
//!
//! The cabinet build imports three device libraries: the HASP dongle
//! (`hasp_windows_x64_28756.dll`, by ordinal), the USB dongle finder
//! (`nbamUsbFinder.dll`) and the card reader (`bngrw.dll`). The hook DLL
//! replaces `bngrw.dll` in the view and exports all three APIs, so the two
//! other import descriptors are renamed to `bngrw.dll`: one module serves every
//! cabinet device and is initialised before any game code runs.
//!
//! Code patches are listed in [`PATCHES`] with the bytes they replace; every
//! one is checked before anything is written. The source file is never
//! modified.

/// SHA-256 of the one supported `wmn6r.exe`.
pub const SHA256: &str = "bf9fded65f5d029deaed5c3b5d06e8035dae70eee2e03c51e3adc2b1444ae2be";

/// The library the hook DLL takes the place of.
pub const HOOK_LIBRARY: &str = "bngrw.dll";
const RENAMED: [&str; 2] = ["hasp_windows_x64_28756.dll", "nbamUsbFinder.dll"];

/// One code change at an image RVA.
pub struct Patch {
    pub rva: u32,
    pub original: &'static [u8],
    pub replacement: &'static [u8],
    /// Only applied for startup diagnostics.
    pub diagnostic: bool,
    pub why: &'static str,
}

pub const PATCHES: &[Patch] = &[Patch {
    // The logger compares each message level with the configured one
    // (`mov r10d,[r9+0x28]; test r10d,r10d; je print`). Always taking the
    // branch prints every level to OutputDebugString.
    rva: 0x0098_91b3,
    original: &[0x74, 0x30],
    replacement: &[0xeb, 0x30],
    diagnostic: true,
    why: "print every log level",
}];

struct Pe<'a> {
    bytes: &'a [u8],
    sections: Vec<(u32, u32, u32, u32)>, // rva, virtual size, raw offset, raw size
    imports: u32,
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| "Truncated PE image".into())
}
fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| "Truncated PE image".into())
}

impl<'a> Pe<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        let pe = u32_at(bytes, 0x3c)? as usize;
        if bytes.get(..2) != Some(b"MZ") || bytes.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err("Not a PE image".into());
        }
        let count = u16_at(bytes, pe + 6)? as usize;
        let optional = pe + 24;
        if u16_at(bytes, optional)? != 0x20b {
            return Err("Expected a PE32+ image".into());
        }
        let table = optional + u16_at(bytes, pe + 20)? as usize;
        let sections = (0..count)
            .map(|i| {
                let s = table + i * 40;
                Ok((
                    u32_at(bytes, s + 12)?,
                    u32_at(bytes, s + 8)?,
                    u32_at(bytes, s + 20)?,
                    u32_at(bytes, s + 16)?,
                ))
            })
            .collect::<Result<_, String>>()?;
        // Data directory 1 (imports) of a PE32+ optional header.
        let imports = u32_at(bytes, optional + 112 + 8)?;
        Ok(Self {
            bytes,
            sections,
            imports,
        })
    }
    fn offset(&self, rva: u32) -> Result<usize, String> {
        self.sections
            .iter()
            .find(|(start, size, _, raw)| rva >= *start && rva - start < (*size).max(*raw))
            .filter(|(start, _, _, raw)| rva - start < *raw)
            .map(|(start, _, offset, _)| (offset + rva - start) as usize)
            .ok_or_else(|| format!("RVA {rva:#x} is outside the image file"))
    }
    /// File offset and length of each import descriptor's library name.
    fn import_names(&self) -> Result<Vec<(usize, String)>, String> {
        let mut out = Vec::new();
        let mut at = self.offset(self.imports)?;
        loop {
            let name = u32_at(self.bytes, at + 12)?;
            if name == 0 {
                return Ok(out);
            }
            let start = self.offset(name)?;
            let end = self.bytes[start..]
                .iter()
                .position(|&b| b == 0)
                .ok_or("Unterminated import name")?;
            out.push((
                start,
                String::from_utf8_lossy(&self.bytes[start..start + end]).into_owned(),
            ));
            at += 20;
        }
    }
}

/// The session's executable: device imports pointed at the hook library and
/// the [`PATCHES`] applied (diagnostic ones only when `diagnostic`).
pub fn prepare(original: &[u8], diagnostic: bool) -> Result<Vec<u8>, String> {
    let pe = Pe::parse(original)?;
    let names = pe.import_names()?;
    let mut image = original.to_vec();
    for wanted in RENAMED {
        let mut found = names.iter().filter(|(_, n)| n.eq_ignore_ascii_case(wanted));
        let (at, name) = found
            .next()
            .ok_or_else(|| format!("wmn6r.exe does not import {wanted}"))?;
        if found.next().is_some() {
            return Err(format!("wmn6r.exe imports {wanted} twice"));
        }
        let slot = &mut image[*at..*at + name.len()];
        slot.fill(0);
        slot[..HOOK_LIBRARY.len()].copy_from_slice(HOOK_LIBRARY.as_bytes());
    }
    if !names
        .iter()
        .any(|(_, n)| n.eq_ignore_ascii_case(HOOK_LIBRARY))
    {
        return Err(format!("wmn6r.exe does not import {HOOK_LIBRARY}"));
    }
    for patch in PATCHES.iter().filter(|p| diagnostic || !p.diagnostic) {
        let at = pe.offset(patch.rva)?;
        let slot = image
            .get_mut(at..at + patch.original.len())
            .ok_or("Patch outside the image")?;
        if slot != patch.original {
            return Err(format!(
                "Unexpected bytes at {:#x} ({}); no patches applied",
                patch.rva, patch.why
            ));
        }
        slot.copy_from_slice(patch.replacement);
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    /// A minimal PE32+ image: one section holding an import directory for
    /// the three device libraries and a patch site.
    fn image() -> Vec<u8> {
        let mut b = vec![0u8; 0x600];
        b[..2].copy_from_slice(b"MZ");
        b[0x3c] = 0x80;
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x86] = 1; // one section
        b[0x94] = 0xf0; // optional header size
        b[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
        // Import directory at RVA 0x1000.
        b[0x98 + 120..0x98 + 124].copy_from_slice(&0x1000u32.to_le_bytes());
        let s = 0x98 + 0xf0;
        b[s + 8..s + 12].copy_from_slice(&0x1000u32.to_le_bytes()); // virtual size
        b[s + 12..s + 16].copy_from_slice(&0x1000u32.to_le_bytes()); // rva
        b[s + 16..s + 20].copy_from_slice(&0x400u32.to_le_bytes()); // raw size
        b[s + 20..s + 24].copy_from_slice(&0x200u32.to_le_bytes()); // raw offset
        let names = [
            (0x1100u32, &b"bngrw.dll\0"[..]),
            (0x1120, b"hasp_windows_x64_28756.dll\0"),
            (0x1140, b"nbamUsbFinder.dll\0"),
        ];
        for (i, (rva, name)) in names.iter().enumerate() {
            let d = 0x200 + i * 20;
            b[d + 12..d + 16].copy_from_slice(&rva.to_le_bytes());
            let at = 0x200 + (*rva - 0x1000) as usize;
            b[at..at + name.len()].copy_from_slice(name);
        }
        b
    }

    #[test]
    fn device_imports_point_at_the_hook_library() {
        let out = super::prepare(&image(), false).unwrap();
        assert_eq!(&out[0x300..0x30a], b"bngrw.dll\0");
        assert_eq!(&out[0x320..0x33b], b"bngrw.dll\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0");
        assert_eq!(&out[0x340..0x352], b"bngrw.dll\0\0\0\0\0\0\0\0\0");
    }

    #[test]
    fn missing_device_import_is_an_error() {
        let mut b = image();
        b[0x340] = b'x';
        assert!(super::prepare(&b, false).is_err());
    }

    #[test]
    fn unexpected_patch_bytes_are_an_error() {
        // The synthetic image does not hold the diagnostic patch site.
        assert!(super::prepare(&image(), true).is_err());
    }
}
