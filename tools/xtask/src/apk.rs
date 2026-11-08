//! The APK: the Rust JNI library, the Kotlin frontend and the runtime assets,
//! built with the SDK's own tools (aapt2, d8, zipalign, apksigner) and no
//! Gradle. Runs on Windows, Linux and macOS hosts; it downloads nothing
//! beyond the pinned Kotlin compiler.
use crate::{
    android::{Sdk, java, kotlin},
    util::{Context, Result, android_build, native, output, root, run, sha256, sha256_file},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

pub fn build() -> Result<()> {
    let root = root();
    let build = android_build();
    let app = root.join("apps/android/app");
    let sdk = Sdk::find()?;
    let kotlinc = kotlin()?;
    let java = java()?;
    let resources = root.join("target/dist/banana-pepper-android-resources");
    let version_code = env(
        "VERSION_CODE",
        &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_secs())
            .to_string(),
    );
    let version_name = env("VERSION_NAME", "0.2.0");
    let app_id = env("APP_ID", "com.bananapepper");
    let release = std::env::var("RELEASE").is_ok_and(|v| v == "1");

    for (path, hint) in [
        (build.join("rootfs.tar.gz"), "cargo xtask android rootfs"),
        (resources.join("banana-pepper.dist"), "cargo xtask android resources (Linux host)"),
    ] {
        if !path.is_file() {
            return Err(format!("Missing {}; run {hint}", path.display()));
        }
    }

    // The JNI library and the tracer that runs every guest process. Both are
    // pure Rust: the NDK only links them.
    run(Command::new("cargo")
        .current_dir(&root)
        .args(["build", "--release", "--locked", "--target", "aarch64-linux-android"])
        .args(["-p", "banana-ffi", "-p", "banana-tracer"])
        .env("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", sdk.clang()))?;

    let work = build.join("kotlin-apk");
    if work.exists() {
        fs::remove_dir_all(&work).ctx(work.display())?;
    }
    let assets = work.join("assets/runtime");
    let tools = assets.join("tools");
    let libs = work.join("lib/arm64-v8a");
    for dir in [&tools, &libs, &work.join("gen"), &work.join("dex"), &work.join("src/com/bananapepper")] {
        fs::create_dir_all(dir).ctx(dir.display())?;
    }
    let tracer = assets.join("banana-tracer");
    fs::copy(root.join("target/aarch64-linux-android/release/banana-tracer"), &tracer).ctx("banana-tracer")?;
    run(Command::new(sdk.tool("llvm-strip")).arg("--strip-unneeded").arg(&tracer))?;
    let marker = b"BANANA_TRACER_VIEW_PROTOCOL_2";
    if !fs::read(&tracer).ctx("banana-tracer")?.windows(marker.len()).any(|w| w == marker) {
        return Err("The built banana-tracer lacks its view protocol marker".into());
    }
    copy_tree(&resources.join("games"), &tools.join("games"))?;
    for name in ["banana-pepper.dist", "artifacts.json", "THIRD-PARTY-NOTICES.txt"] {
        fs::copy(resources.join(name), tools.join(name)).ctx(name)?;
    }
    if build.join("box64").is_file() {
        fs::copy(build.join("box64"), tools.join("box64")).ctx("box64")?;
    }
    for name in ["wine", "x64-libs"] {
        if build.join(name).is_dir() {
            copy_tree(&build.join(name), &tools.join(name))?;
        }
    }
    // Optional redistributable ABI dependencies come from per-user storage,
    // never from game or runtime trees.
    let legacy = std::env::var_os("BANANA_ANDROID_LEGACY32").map(PathBuf::from).or_else(|| {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(data.join("banana-pepper/dependencies/legacy32"))
    });
    let legacy_out = tools.join("dependencies/legacy32");
    fs::create_dir_all(&legacy_out).ctx("legacy32")?;
    if let Some(legacy) = legacy.filter(|p| p.is_dir()) {
        copy_tree(&legacy, &legacy_out)?;
    }
    let rootfs = build.join("rootfs.tar.gz");
    if fs::hard_link(&rootfs, assets.join("rootfs.tar.gz")).is_err() {
        fs::copy(&rootfs, assets.join("rootfs.tar.gz")).ctx("rootfs")?;
    }
    fs::write(assets.join("version.txt"), format!("{}\n", &sha256_file(&rootfs)?[..16])).ctx("version")?;
    fs::write(assets.join("tools-version.txt"), format!("{}\n", tools_version(&tools)?)).ctx("version")?;

    fs::copy(
        root.join("target/aarch64-linux-android/release/libbanana_ffi.so"),
        libs.join("libbanana_ffi.so"),
    )
    .ctx("libbanana_ffi.so")?;
    run(Command::new(sdk.tool("llvm-strip"))
        .arg("--strip-unneeded")
        .arg(libs.join("libbanana_ffi.so")))?;
    fs::write(
        work.join("src/com/bananapepper/BuildInfo.kt"),
        format!(
            "package com.bananapepper\nobject BuildInfo {{ const val VERSION_CODE = {version_code}; const val VERSION_NAME = \"{version_name}\" }}\n"
        ),
    )
    .ctx("BuildInfo.kt")?;

    // Resources and manifest.
    let aapt2 = sdk.build_tool("aapt2");
    run(Command::new(&aapt2)
        .arg("compile")
        .arg("--dir")
        .arg(native(&app.join("res")))
        .arg("-o")
        .arg(native(&work.join("res.zip"))))?;
    let mut link = Command::new(&aapt2);
    link.arg("link")
        .arg("-o")
        .arg(native(&work.join("base.apk")))
        .arg("-I")
        .arg(native(&sdk.platform))
        .arg("--manifest")
        .arg(native(&app.join("AndroidManifest.xml")))
        .arg(native(&work.join("res.zip")))
        .args(["--rename-manifest-package", &app_id]);
    if !release {
        link.arg("--debug-mode");
    }
    link.arg("--java")
        .arg(native(&work.join("gen")))
        // Assets are appended below: aapt2 on Windows names nested assets
        // with backslashes, which the app cannot open.
        .args(["--min-sdk-version", "26", "--target-sdk-version", "28"])
        .args(["--version-code", &version_code, "--version-name", &version_name]);
    run(&mut link)?;

    // Kotlin to dex.
    let mut sources = String::new();
    for dir in [app.join("src"), work.join("src")] {
        for file in files(&dir)? {
            if file.extension().is_some_and(|e| e == "kt") {
                sources.push_str(&format!("\"{}\"\n", native(&file)));
            }
        }
    }
    fs::write(work.join("sources.txt"), sources).ctx("sources")?;
    run(Command::new(&java)
        .args(["-Xmx2g", "-cp"])
        .arg(native(&kotlinc.join("lib/kotlin-compiler.jar")))
        .arg("org.jetbrains.kotlin.cli.jvm.K2JVMCompiler")
        .arg("-kotlin-home")
        .arg(native(&kotlinc))
        .arg(format!("@{}", native(&work.join("sources.txt"))))
        .arg("-classpath")
        .arg(native(&sdk.platform))
        .args(["-jvm-target", "1.8", "-no-reflect", "-include-runtime", "-d"])
        .arg(native(&work.join("app.jar"))))?;
    run(Command::new(&java)
        .arg("-cp")
        .arg(native(&sdk.build_tools.join("lib/d8.jar")))
        .args(["com.android.tools.r8.D8", "--min-api", "26", "--lib"])
        .arg(native(&sdk.platform))
        .arg("--output")
        .arg(native(&work.join("dex")))
        .arg(native(&work.join("app.jar"))))?;

    // Native libraries and dex go in stored, then align and sign.
    let unsigned = work.join("unsigned.apk");
    fs::copy(work.join("base.apk"), &unsigned).ctx("apk")?;
    add_entries(&unsigned, &work)?;
    let aligned = work.join("aligned.apk");
    run(Command::new(sdk.build_tool("zipalign"))
        .args(["-P", "16", "-f", "4"])
        .arg(&unsigned)
        .arg(&aligned))?;
    let keystore = root.join("apps/android/debug.keystore");
    if !keystore.is_file() {
        let keytool = java.with_file_name(if cfg!(windows) { "keytool.exe" } else { "keytool" });
        run(Command::new(keytool)
            .arg("-genkeypair")
            .arg("-keystore")
            .arg(native(&keystore))
            .args(["-storepass", "android", "-keypass", "android", "-alias", "bananapepper"])
            .args(["-keyalg", "RSA", "-keysize", "2048", "-validity", "10000"])
            .args(["-dname", "CN=Banana Pepper Debug"]))?;
    }
    let apk = build.join("BananaPepper.apk");
    let signer = native(&sdk.build_tools.join("lib/apksigner.jar"));
    run(Command::new(&java)
        .args(["-jar", &signer, "sign", "--ks"])
        .arg(native(&keystore))
        .args(["--ks-pass", "pass:android", "--key-pass", "pass:android", "--out"])
        .arg(native(&apk))
        .arg(native(&aligned)))?;
    output(Command::new(&java).args(["-jar", &signer, "verify"]).arg(native(&apk)))?;
    println!(
        "APK ready: {} ({} MB)",
        apk.display(),
        fs::metadata(&apk).ctx("apk")?.len() >> 20
    );
    Ok(())
}

/// Every regular file below `dir`, sorted.
fn files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).ctx(dir.display())? {
            let path = entry.ctx(dir.display())?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Copy a directory tree, following links (assets cannot hold them).
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for file in files(from)? {
        let target = to.join(file.strip_prefix(from).ctx("copy")?);
        fs::create_dir_all(target.parent().unwrap()).ctx(target.display())?;
        fs::copy(&file, &target).ctx(file.display())?;
    }
    Ok(())
}

/// The installer's tools stamp: a digest of every tools file and its hash.
fn tools_version(tools: &Path) -> Result<String> {
    let mut listing = String::new();
    for file in files(tools)? {
        let relative = file.strip_prefix(tools).ctx("tools")?.to_string_lossy().replace('\\', "/");
        listing.push_str(&format!("{}  ./{relative}\n", sha256_file(&file)?));
    }
    Ok(sha256(listing.as_bytes())[..16].to_owned())
}

/// One file to add to the APK under a '/'-separated name.
struct Addition {
    name: String,
    data: Vec<u8>,
    /// Deflate it; native libraries, dex and pre-compressed assets are stored.
    compress: bool,
}

/// Append `assets/**`, `lib/**` and `classes*.dex` to the APK. The existing
/// entries and their central directory records are kept byte for byte
/// (zipalign compares local and central headers).
fn add_entries(apk: &Path, work: &Path) -> Result<()> {
    let mut entries = Vec::new();
    let relative = |path: &Path| -> Result<String> {
        Ok(path.strip_prefix(work).ctx("apk")?.to_string_lossy().replace('\\', "/"))
    };
    for asset in files(&work.join("assets"))? {
        let name = relative(&asset)?;
        // As aapt2's `-0 gz -0 so -0 2`: archives and libraries stay stored.
        let compress = ![".gz", ".so", "2"].iter().any(|s| name.ends_with(s));
        entries.push(Addition { name, data: fs::read(&asset).ctx(asset.display())?, compress });
    }
    for library in files(&work.join("lib"))? {
        let name = relative(&library)?;
        entries.push(Addition { name, data: fs::read(&library).ctx(library.display())?, compress: false });
    }
    for dex in files(&work.join("dex"))? {
        let name = dex.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("classes") && name.ends_with(".dex") {
            entries.push(Addition { name, data: fs::read(&dex).ctx(dex.display())?, compress: false });
        }
    }
    let bytes = fs::read(apk).ctx(apk.display())?;
    fs::write(apk, append(&bytes, &entries)?).ctx(apk.display())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut hasher = flate2::Crc::new();
    hasher.update(bytes);
    hasher.sum()
}

/// A zip with `entries` after the existing ones.
fn append(zip: &[u8], entries: &[Addition]) -> Result<Vec<u8>> {
    let u16_at = |o: usize| u16::from_le_bytes([zip[o], zip[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(zip[o..o + 4].try_into().unwrap());
    let end = (0..zip.len().saturating_sub(21))
        .rev()
        .find(|&o| zip[o..o + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or("APK has no end of central directory")?;
    let count = u16_at(end + 10);
    let (directory_size, directory_offset) = (u32_at(end + 12) as usize, u32_at(end + 16) as usize);
    let directory = zip
        .get(directory_offset..directory_offset + directory_size)
        .ok_or("APK central directory is out of range")?;
    // The app opens assets by '/'-separated names.
    let mut record = 0;
    while record + 46 <= directory.len() && directory[record..record + 4] == [0x50, 0x4b, 0x01, 0x02] {
        let field = |o: usize| u16::from_le_bytes([directory[record + o], directory[record + o + 1]]) as usize;
        let (name, extra, comment) = (field(28), field(30), field(32));
        let entry = String::from_utf8_lossy(&directory[record + 46..record + 46 + name]);
        if entry.contains('\\') {
            return Err(format!("APK entry {entry} uses a Windows path separator"));
        }
        record += 46 + name + extra + comment;
    }
    let mut out = zip[..directory_offset].to_vec();
    let mut records = Vec::new();
    for entry in entries {
        let data = &entry.data;
        let deflated = if entry.compress {
            use std::io::Write;
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(9));
            encoder.write_all(data).ctx("deflate")?;
            Some(encoder.finish().ctx("deflate")?).filter(|d| d.len() < data.len())
        } else {
            None
        };
        let (method, body) = match &deflated {
            Some(d) => (8u16, d),
            None => (0u16, data),
        };
        let crc = crc32(data);
        let size = u32::try_from(data.len()).ctx("zip entry size")?;
        let packed = u32::try_from(body.len()).ctx("zip entry size")?;
        let offset = u32::try_from(out.len()).ctx("zip offset")?;
        let name_bytes = entry.name.as_bytes();
        let common = |v: &mut Vec<u8>| {
            v.extend_from_slice(&20u16.to_le_bytes()); // version needed
            v.extend_from_slice(&0u16.to_le_bytes()); // flags
            v.extend_from_slice(&method.to_le_bytes());
            v.extend_from_slice(&0u16.to_le_bytes()); // time
            v.extend_from_slice(&0x21u16.to_le_bytes()); // date: 1980-01-01
            v.extend_from_slice(&crc.to_le_bytes());
            v.extend_from_slice(&packed.to_le_bytes());
            v.extend_from_slice(&size.to_le_bytes());
            v.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        };
        out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        common(&mut out);
        out.extend_from_slice(&0u16.to_le_bytes()); // extra length
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(body);
        records.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        records.extend_from_slice(&20u16.to_le_bytes()); // version made by
        common(&mut records);
        records.extend_from_slice(&[0; 8]); // extra, comment, disk, internal attributes
        records.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        records.extend_from_slice(&offset.to_le_bytes());
        records.extend_from_slice(name_bytes);
    }
    let new_offset = u32::try_from(out.len()).ctx("zip offset")?;
    out.extend_from_slice(directory);
    out.extend_from_slice(&records);
    let total = count
        .checked_add(u16::try_from(entries.len()).ctx("zip entries")?)
        .ok_or("Too many zip entries")?;
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&((directory.len() + records.len()) as u32).to_le_bytes());
    out.extend_from_slice(&new_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn appended_entries_read_back_alongside_existing_ones() {
        use std::io::{Read, Write};
        let mut base = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut base);
            zip.start_file("AndroidManifest.xml", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"manifest").unwrap();
            zip.finish().unwrap();
        }
        let text = b"runtime ".repeat(64);
        let out = super::append(
            base.get_ref(),
            &[
                super::Addition {
                    name: "lib/arm64-v8a/libx.so".into(),
                    data: b"\x7fELF".to_vec(),
                    compress: false,
                },
                super::Addition {
                    name: "assets/runtime/tools/notes.txt".into(),
                    data: text.clone(),
                    compress: true,
                },
            ],
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(out.clone())).unwrap();
        assert_eq!(zip.len(), 3);
        for (name, method, expected) in [
            ("lib/arm64-v8a/libx.so", zip::CompressionMethod::Stored, &b"\x7fELF"[..]),
            ("assets/runtime/tools/notes.txt", zip::CompressionMethod::Deflated, &text[..]),
        ] {
            let mut entry = zip.by_name(name).unwrap();
            assert_eq!(entry.compression(), method, "{name}");
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, expected, "{name}");
        }
        // Windows-separated names in the original are refused.
        let mut windows = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut windows);
            zip.start_file("assets/runtime\\x", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.finish().unwrap();
        }
        assert!(super::append(windows.get_ref(), &[]).is_err());
    }
}
