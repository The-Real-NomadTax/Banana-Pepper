//! Android platform requirements; preparation uses the common compatibility plan.
use banana_api::AndroidProfile;
pub fn profile() -> AndroidProfile {
    AndroidProfile {
        executable: "sdaemon.exe",
        requirements: &[
            "opt/arcade/games/super-cars/windows-x86/arcade_wine_hook.dll",
            "usr/local/bin/box64",
            "usr/bin/Xvfb",
            "usr/bin/pulseaudio",
            "opt/arcade/wine/bin/wine",
            "opt/arcade/x64-libs/libgcc_s.so.1",
            "opt/arcade/wine/lib/wine/x86_64-unix/ntdll.so",
        ],
        status: "Experimental Wine WoW64 runtime",
        verified: false,
    }
}

/// Disable the cabinet Ethernet link without changing other operator settings.
/// Only the Android plan applies it (desktop Wine has no cabinet network).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn cabinet_preferences(bytes: &[u8]) -> Result<banana_api::BinaryConfig, String> {
    if bytes.get(..4) != Some(b"320P") || (bytes.len() - 4) % 128 != 0 {
        return Err("Unsupported Super Cars preferences format".into());
    }
    let mut matches = bytes[4..]
        .chunks_exact(128)
        .enumerate()
        .filter(|(_, r)| r[8..40].starts_with(b"Network enabled\0"));
    let (index, record) = matches.next().ok_or("Missing cabinet-link preference")?;
    if matches.next().is_some()
        || u32::from_le_bytes(record[..4].try_into().unwrap()) != 41
        || u32::from_le_bytes(record[4..8].try_into().unwrap()) != 128
    {
        return Err("Ambiguous or unsupported cabinet-link preference".into());
    }
    let base = 4 + index * 128;
    Ok(banana_api::BinaryConfig {
        path: "prefs".into(),
        checks: vec![(0, b"320P".to_vec()), (base, record[..40].to_vec())],
        edits: vec![banana_api::ByteEdit {
            offset: base + 40,
            accepted: vec![0u32.to_le_bytes().to_vec(), 1u32.to_le_bytes().to_vec()],
            replacement: 0u32.to_le_bytes().to_vec(),
        }],
    })
}
#[cfg(test)]
mod tests {
    #[test]
    fn network_edit_is_guarded_and_preserves_other_operator_fields() {
        let mut bytes = vec![0x55; 132];
        bytes[..4].copy_from_slice(b"320P");
        bytes[4..8].copy_from_slice(&41u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&128u32.to_le_bytes());
        bytes[12..28].copy_from_slice(b"Network enabled\0");
        bytes[44..48].copy_from_slice(&1u32.to_le_bytes());
        let config = super::cabinet_preferences(&bytes).unwrap();
        assert_eq!(config.edits[0].offset, 44);
        assert_eq!(config.edits[0].replacement, [0, 0, 0, 0]);
        bytes[8] = 0;
        assert!(super::cabinet_preferences(&bytes).is_err());
        assert!(super::cabinet_preferences(&[]).is_err());
    }
}
