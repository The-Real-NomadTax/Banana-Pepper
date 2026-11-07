use banana_api::{AndroidLaunch, AndroidProfile};
use std::path::Path;
pub const SOURCE_SHA256: &str = "0cddbdbf1d107d27dc668897d7851b6ac96b4349320551d800504b1c50b195ba";

fn single_cabinet_preferences(bytes: &mut [u8]) -> Result<(), String> {
    if bytes.get(..4) != Some(b"320P") || (bytes.len() - 4) % 128 != 0 {
        return Err("Unsupported Super Cars preferences format".into());
    }
    let records: Vec<_> = bytes[4..].chunks_exact(128).enumerate().filter_map(|(index, record)| {
        (record[8..40].starts_with(b"Network enabled\0")).then_some((index, record))
    }).collect();
    if records.len() != 1 {
        return Err("Missing or ambiguous Super Cars cabinet-link preference".into());
    }
    let (index, record) = records[0];
    if u32::from_le_bytes(record[..4].try_into().unwrap()) != 41
        || u32::from_le_bytes(record[4..8].try_into().unwrap()) != 128
        || u32::from_le_bytes(record[40..44].try_into().unwrap()) > 1 {
        return Err("Unsupported Super Cars cabinet-link preference".into());
    }
    let offset = 4 + index * 128 + 40;
    bytes[offset..offset+4].fill(0);
    Ok(())
}
pub fn profile() -> AndroidProfile {
    AndroidProfile {
        executable: "sdaemon.exe",
        requirements: &[
            "usr/local/bin/box64",
            "usr/local/bin/afbridge",
            "usr/bin/pulseaudio",
            "usr/local/bin/afaudio",
            "opt/arcade/wine/bin/wine",
            "opt/arcade/x64-libs/libgcc_s.so.1",
            "opt/arcade/wine/lib/wine/x86_64-unix/ntdll.so",
        ],
        status: "Experimental Wine WoW64 runtime",
        verified: false,
    }
}

pub fn prepare(source: &Path, work: &Path, tools: &Path) -> Result<AndroidLaunch, String> {
    use sha2::{Digest, Sha256};
    let data = std::fs::read(source.join("sdaemon.exe")).map_err(|e| e.to_string())?;
    if format!("{:x}", Sha256::digest(data)) != SOURCE_SHA256 {
        return Err("Unsupported Super Cars build".into());
    }
    // A private copy keeps Wine's writes and cabinet audits away from originals.
    banana_runtime::android::seed(source, &work.join("game"))?;
    // A phone is one cabinet. Use the game's existing operator setting rather
    // than waiting for another cabinet's Ethernet link during initialization.
    let prefs = work.join("game/prefs");
    let mut bytes = std::fs::read(&prefs).map_err(|e| e.to_string())?;
    single_cabinet_preferences(&mut bytes)?;
    std::fs::write(prefs, bytes).map_err(|e| e.to_string())?;
    std::fs::copy(
        tools.join("arcade_wine_hook.dll"),
        work.join("game/dinput8.dll"),
    )
    .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(work.join("prefix/drive_c/rawart")).map_err(|e| e.to_string())?;
    let mut plan = AndroidLaunch::new(
        "/work/prefix/drive_c/rawart",
        &[
            "box64",
            "/opt/arcade/wine/bin/wine",
            "explorer",
            "/desktop=SuperCars,1360x768",
            "C:\\rawart\\sdaemon.exe",
            "-noerr",
            "-res13",
        ],
    );
    plan.bind(work, "/work");
    plan.bind(work.join("game"), "/work/prefix/drive_c/rawart");
    plan.env("BOX64_LD_LIBRARY_PATH", "/opt/arcade/x64-libs");
    plan.env("BOX64_ANDROID_NOEXEC", "1");
    plan.env("AF_PULSE_AUDIO", "1");
    plan.env("ARCADE_RESOLUTION", "1360x768");
    plan.env("AF_FRAME_FPS", "30");
    plan.env("WINEPREFIX", "/work/prefix");
    plan.env(
        "WINEDLLOVERRIDES",
        "dinput8=n,b;d3d8,d3d9=b;mscoree,mshtml=",
    );
    plan.env(
        "ARCADE_SUPERCARS_KEYS",
        &banana_input::controls::Bindings::default().env_value("super_cars")?,
    );
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn preferences() -> Vec<u8> {
        let mut bytes = vec![0x55; 4 + 128 * 2];
        bytes[..4].copy_from_slice(b"320P");
        let record = &mut bytes[132..];
        record[..4].copy_from_slice(&41u32.to_le_bytes());
        record[4..8].copy_from_slice(&128u32.to_le_bytes());
        record[8..24].copy_from_slice(b"Network enabled\0");
        record[40..44].copy_from_slice(&1u32.to_le_bytes());
        bytes
    }
    #[test]
    fn single_cabinet_keeps_other_operator_settings() {
        let mut bytes = preferences();
        let mut expected = bytes.clone();
        expected[172..176].fill(0);
        single_cabinet_preferences(&mut bytes).unwrap();
        assert_eq!(bytes, expected);
        single_cabinet_preferences(&mut bytes).unwrap();
        assert_eq!(bytes, expected);
    }
    #[test]
    fn unknown_preference_layout_is_unchanged() {
        let mut bytes = preferences();
        bytes[136..140].fill(0);
        let original = bytes.clone();
        assert!(single_cabinet_preferences(&mut bytes).is_err());
        assert_eq!(bytes, original);
        assert!(single_cabinet_preferences(&mut []).is_err());
    }
}
