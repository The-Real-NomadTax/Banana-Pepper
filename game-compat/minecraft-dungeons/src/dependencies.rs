//! Minecraft's declared Linux libraries and their local preparation.
use banana_runtime::{
    dependencies::{self, download, extract_member, write_library},
    process::checked,
};
use std::{fs, path::Path, process::Command};
const FMOD_URL: &str =
    "https://downloads.sourceforge.net/project/manualinux/Doomsday/fmodstudioapi11020linux.tar.gz";
const FMOD_HASH: &str = "53231260b694286807995714bb11011afe08361eb4873bc75cf335873e167137";
pub fn setup(output: &Path) -> Result<(), String> {
    let cache = output.join("packages");
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    fs::create_dir_all(output.join("lib")).map_err(|e| e.to_string())?;
    let libraries = output.join("libraries");
    fs::create_dir_all(&libraries).map_err(|e| e.to_string())?;
    if find("pacman") {
        let metadata = checked(Command::new("pacman").args([
            "-Sp",
            "--print-format",
            "%n %v %h %l",
            "wireless_tools",
        ]))?;
        let metadata = String::from_utf8(metadata.stdout).map_err(|e| e.to_string())?;
        let (version, digest, url) = wireless_record(&metadata)?;
        let archive = cache.join(format!("wireless_tools-{digest}.pkg.tar.zst"));
        download(&url, &digest, &archive, 16 * 1024 * 1024)?;
        let library = extract_member(&archive, "usr/lib/libiw.so.30")?;
        write_library(&output.join("lib/libiw.so.30"), &library, 64)?;
        fs::write(output.join("wireless-source.json"),serde_json::to_vec_pretty(&serde_json::json!({"package":"wireless_tools","version":version,"url":url,"archive_sha256":digest,"library_sha256":dependencies::sha256(&library)})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    } else {
        if !find("apt-get") || !find("dpkg-deb") {
            return Err("Automatic dependency setup requires apt/dpkg or pacman; libraries may also be supplied in runtime/minecraft/lib".into());
        }
        for package in [
            "libglu1-mesa",
            "libopengl0",
            "liblapack3",
            "libblas3",
            "libgfortran5",
            "libquadmath0",
            "libiw30t64",
            "libpcsclite1",
        ] {
            checked(
                Command::new("apt-get")
                    .args(["download", package])
                    .current_dir(&cache),
            )?;
        }
        for e in fs::read_dir(&cache).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            if e.path().extension().is_some_and(|x| x == "deb") {
                checked(
                    Command::new("dpkg-deb")
                        .arg("--extract")
                        .arg(e.path())
                        .arg(&libraries),
                )?;
            }
        }
    }
    let archive = cache.join("fmodstudioapi11020linux.tar.gz");
    download(FMOD_URL, FMOD_HASH, &archive, 64 * 1024 * 1024)?;
    let mut records = serde_json::Map::new();
    for (name, member) in [
        ("libfmod.so.10", "api/lowlevel/lib/x86_64/libfmod.so.10.20"),
        (
            "libfmodstudio.so.10",
            "api/studio/lib/x86_64/libfmodstudio.so.10.20",
        ),
        ("FMOD-LICENSE.TXT", "doc/LICENSE.TXT"),
    ] {
        let bytes = extract_member(&archive, &format!("fmodstudioapi11020linux/{member}"))?;
        let path = output.join("lib").join(name);
        if name.ends_with(".10") {
            write_library(&path, &bytes, 64)?;
        } else {
            fs::write(path, &bytes).map_err(|e| e.to_string())?;
        }
        records.insert(name.into(), dependencies::sha256(&bytes).into());
    }
    fs::write(output.join("fmod-source.json"),serde_json::to_vec_pretty(&serde_json::json!({"url":FMOD_URL,"archive_sha256":FMOD_HASH,"files":records,"note":"Local SDK cache; not included in application distributions"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    Ok(())
}
fn find(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}
fn wireless_record(text: &str) -> Result<(String, String, String), String> {
    let lines: Vec<_> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() != 1 {
        return Err("Expected one wireless_tools package record".into());
    }
    let p: Vec<_> = lines[0].split_whitespace().collect();
    if p.len() != 4
        || p[0] != "wireless_tools"
        || p[2].len() != 64
        || !p[2].bytes().all(|b| b.is_ascii_hexdigit())
        || !p[3].starts_with("https://")
    {
        return Err("Invalid wireless_tools package metadata".into());
    }
    Ok((p[1].into(), p[2].into(), p[3].into()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_metadata_is_validated_before_downloading() {
        assert!(wireless_record("wireless_tools 30 bad https://example.invalid").is_err());
        assert!(
            wireless_record(&format!(
                "wireless_tools 30 {} http://example.invalid",
                "0".repeat(64)
            ))
            .is_err()
        );
        assert!(
            wireless_record(&format!(
                "wireless_tools 30 {} https://example.invalid",
                "0".repeat(64)
            ))
            .is_ok()
        );
    }
}
