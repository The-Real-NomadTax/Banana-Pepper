//! Verified local dependency staging. No package installation or shell pipelines.
use crate::process::checked;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn download(url: &str, digest: &str, destination: &Path, limit: u64) -> Result<(), String> {
    if !url.starts_with("https://")
        || digest.len() != 64
        || !digest.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid dependency URL or SHA-256".into());
    }
    if fs::read(destination).is_ok_and(|b| b.len() as u64 <= limit && sha256(&b) == digest) {
        return Ok(());
    }
    fs::create_dir_all(destination.parent().ok_or("Dependency has no parent")?)
        .map_err(|e| e.to_string())?;
    let temporary = destination.with_extension("download");
    let result = (|| {
        let mut next = url.to_owned();
        for _ in 0..3 {
            checked(
                Command::new("curl")
                    .args([
                        "--fail",
                        "--location",
                        "--proto",
                        "=https",
                        "--proto-redir",
                        "=https",
                        "--max-time",
                        "120",
                        "--max-filesize",
                        &limit.to_string(),
                        "--user-agent",
                        "BananaPepper/0.1",
                        "--output",
                    ])
                    .arg(&temporary)
                    .arg(&next),
            )?;
            let bytes = fs::read(&temporary).map_err(|e| e.to_string())?;
            if bytes.len() as u64 <= limit && sha256(&bytes) == digest {
                return fs::rename(&temporary, destination).map_err(|e| e.to_string());
            }
            // Some archive mirrors return an HTML download-selection page.
            // Follow only a query on the original HTTPS URL, then recheck its hash.
            let page = String::from_utf8_lossy(
                bytes.get(..bytes.len().min(512 * 1024)).unwrap_or_default(),
            );
            let prefix = format!("{url}?");
            if let Some(start) = page.find(&prefix) {
                let tail = &page[start..];
                let end = tail
                    .find(|c: char| c.is_whitespace() || ['\"', '\'', '<', '>'].contains(&c))
                    .unwrap_or(tail.len());
                next = tail[..end].replace("&amp;", "&");
            } else {
                break;
            }
        }
        Err("Dependency checksum/size mismatch; existing libraries were preserved".into())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
pub fn extract_member(archive: &Path, member: &str) -> Result<Vec<u8>, String> {
    Ok(checked(
        Command::new("tar")
            .args(["-xOf"])
            .arg(tar_path(archive)?)
            .arg(member),
    )?
    .stdout)
}

fn tar_path(archive: &Path) -> Result<PathBuf, String> {
    if archive.extension().is_none_or(|e| e != "zst") {
        return Ok(archive.to_owned());
    }
    let compressed = fs::read(archive).map_err(|e| e.to_string())?;
    let bytes = decode_zstd(&compressed, 512 * 1024 * 1024)?;
    let path = archive.with_extension("decoded.tar");
    fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}
fn decode_zstd(bytes: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    struct Decoder(*mut zstd_sys::ZSTD_DStream);
    impl Drop for Decoder {
        fn drop(&mut self) {
            unsafe {
                zstd_sys::ZSTD_freeDStream(self.0);
            }
        }
    }
    let stream = Decoder(unsafe { zstd_sys::ZSTD_createDStream() });
    if stream.0.is_null() {
        return Err("Cannot allocate Zstandard decoder".into());
    }
    let init = unsafe { zstd_sys::ZSTD_initDStream(stream.0) };
    if unsafe { zstd_sys::ZSTD_isError(init) } != 0 {
        return Err("Cannot initialize Zstandard decoder".into());
    }
    let mut input = zstd_sys::ZSTD_inBuffer {
        src: bytes.as_ptr().cast(),
        size: bytes.len(),
        pos: 0,
    };
    let mut output = Vec::new();
    let mut buffer = [0u8; 65536];
    loop {
        let before = input.pos;
        let mut chunk = zstd_sys::ZSTD_outBuffer {
            dst: buffer.as_mut_ptr().cast(),
            size: buffer.len(),
            pos: 0,
        };
        let remaining =
            unsafe { zstd_sys::ZSTD_decompressStream(stream.0, &mut chunk, &mut input) };
        if unsafe { zstd_sys::ZSTD_isError(remaining) } != 0 {
            return Err(format!(
                "Invalid Zstandard archive: {}",
                unsafe { std::ffi::CStr::from_ptr(zstd_sys::ZSTD_getErrorName(remaining)) }
                    .to_string_lossy()
            ));
        }
        if output.len().saturating_add(chunk.pos) > limit {
            return Err("Dependency archive expands beyond its size limit".into());
        }
        output.extend_from_slice(&buffer[..chunk.pos]);
        if remaining == 0 && input.pos == input.size {
            return Ok(output);
        }
        if input.pos == before && chunk.pos == 0 {
            return Err("Truncated Zstandard archive".into());
        }
    }
}
pub fn write_library(path: &Path, bytes: &[u8], bits: u8) -> Result<(), String> {
    if !bytes.starts_with(&[0x7f, b'E', b'L', b'F', if bits == 64 { 2 } else { 1 }]) {
        return Err(format!("{} is not ELF{bits}", path.display()));
    }
    fs::create_dir_all(path.parent().ok_or("Library has no parent")?).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("preparing");
    fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    fs::rename(temporary, path).map_err(|e| e.to_string())
}
pub fn legacy_directory(project: &Path) -> PathBuf {
    project.join("dependencies/legacy32")
}
pub fn setup_legacy(project: &Path) -> Result<(), String> {
    let cache = project.join("dependencies/legacy32-packages");
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let physx = cache.join("physx.tar.gz");
    download(
        "https://developer.download.nvidia.com/PhysX/2.8.1/PhysX_2.8.1_SDK_CoreLinux_deb.tar.gz",
        "2a4822f6f5ba9c00993e529392c6fd957bb0c5e07518e9a08bae9ee502428913",
        &physx,
        512 * 1024 * 1024,
    )?;
    let theora = cache.join("theora.pkg.tar.zst");
    download(
        "https://archive.archlinux.org/packages/l/lib32-libtheora/lib32-libtheora-1.1.1-13-x86_64.pkg.tar.zst",
        "966f9063faee25ca02f8f7c92989c9f371eea8d526cea9923326f782778248e5",
        &theora,
        32 * 1024 * 1024,
    )?;
    let libraries = legacy_directory(project);
    for package in [
        "libphysx-2.8.1_4_i386.deb",
        "libphysx-common_2.8.1-4_i386.deb",
    ] {
        let deb = cache.join(package);
        fs::write(&deb, extract_member(&physx, package)?).map_err(|e| e.to_string())?;
        let tar = cache.join(format!("{package}.data.tar.gz"));
        fs::write(
            &tar,
            checked(Command::new("ar").arg("p").arg(&deb).arg("data.tar.gz"))?.stdout,
        )
        .map_err(|e| e.to_string())?;
        let listing = checked(Command::new("tar").arg("-tf").arg(&tar))?;
        for entry in String::from_utf8_lossy(&listing.stdout).lines() {
            let name = Path::new(entry)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            if [
                "libPhysXLoader.so.1",
                "libPhysXCore.so.1",
                "libNxCooking.so.1",
            ]
            .contains(&name.as_ref())
            {
                write_library(
                    &libraries.join(name.as_ref()),
                    &extract_member(&tar, entry)?,
                    32,
                )?;
            }
        }
    }
    let listing = checked(Command::new("tar").arg("-tf").arg(tar_path(&theora)?))?;
    let text = String::from_utf8_lossy(&listing.stdout);
    let mut found = false;
    for member in text.lines() {
        let Some(name) = Path::new(member).file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("libtheora") {
            continue;
        }
        let Some((base, version)) = name.split_once(".so.") else {
            continue;
        };
        if !version.contains('.') {
            continue;
        }
        let major = version
            .split('.')
            .next()
            .ok_or("Invalid Theora library version")?;
        let soname = format!("{base}.so.{major}");
        write_library(
            &libraries.join(&soname),
            &extract_member(&theora, member)?,
            32,
        )?;
        found |= soname == "libtheora.so.0";
    }
    if !found {
        return Err("Theora archive is missing its versioned library".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zstandard_decoder_needs_no_external_program_and_enforces_bounds() {
        let data = b"synthetic dependency archive";
        let bound = unsafe { zstd_sys::ZSTD_compressBound(data.len()) };
        let mut compressed = vec![0u8; bound];
        let size = unsafe {
            zstd_sys::ZSTD_compress(
                compressed.as_mut_ptr().cast(),
                compressed.len(),
                data.as_ptr().cast(),
                data.len(),
                1,
            )
        };
        assert_eq!(unsafe { zstd_sys::ZSTD_isError(size) }, 0);
        compressed.truncate(size);
        assert_eq!(decode_zstd(&compressed, 1024).unwrap(), data);
        assert!(decode_zstd(&compressed, 1).is_err());
        assert!(decode_zstd(&compressed[..compressed.len() - 1], 1024).is_err());
        assert!(decode_zstd(b"invalid", 1024).is_err());
    }
    #[test]
    fn invalid_library_never_replaces_a_working_copy() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("lib.so");
        fs::write(&p, b"existing").unwrap();
        assert!(write_library(&p, b"corrupt", 64).is_err());
        assert_eq!(fs::read(p).unwrap(), b"existing");
    }
    #[test]
    fn invalid_download_metadata_is_rejected_before_network_io() {
        assert!(
            download(
                "http://example.invalid",
                "invalid",
                Path::new("/nonexistent"),
                1
            )
            .is_err()
        );
    }
}

/// Execute a declared dependency recipe without installing system packages.
pub fn prepare(spec: &banana_api::Dependency, root: &Path, cache: &Path) -> Result<(), String> {
    let recipe = spec.recipe.as_ref().ok_or_else(|| {
        format!(
            "Dependency {} must be prepared using the library setup command",
            spec.id
        )
    })?;
    if !recipe.deb.is_empty() && !cfg!(target_os = "linux") {
        return Err(
            "This dependency recipe requires Linux apt/dpkg; no system packages are installed"
                .into(),
        );
    }
    fs::create_dir_all(root.join("libraries")).map_err(|e| e.to_string())?;
    fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    for package in recipe.deb {
        checked(
            Command::new("apt-get")
                .args(["download", package])
                .current_dir(cache),
        )?;
    }
    for entry in fs::read_dir(cache).map_err(|e| e.to_string())? {
        let p = entry.map_err(|e| e.to_string())?.path();
        if p.extension().is_some_and(|e| e == "deb") {
            checked(
                Command::new("dpkg-deb")
                    .arg("--extract")
                    .arg(p)
                    .arg(root.join("libraries")),
            )?;
        }
    }
    for archive in &recipe.downloads {
        let input = cache.join(format!("{}.archive", archive.sha256));
        download(archive.url, archive.sha256, &input, archive.limit)?;
        for (member, dest) in &archive.members {
            banana_content::path(dest).map_err(|e| e.to_string())?;
            let bytes = extract_member(&input, member)?;
            let target = root.join(dest);
            fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::write(target, bytes).map_err(|e| e.to_string())?;
        }
    }
    fs::write(root.join("prepared.json"),serde_json::to_vec_pretty(&serde_json::json!({"id":spec.id,"packages":recipe.deb,"downloads":recipe.downloads.iter().map(|d|d.sha256).collect::<Vec<_>>()})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    Ok(())
}
