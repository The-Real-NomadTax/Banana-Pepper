//! `cargo xtask android …`: every input of the APK, then the APK itself.
use crate::{
    deb::{self, Node, Tree, Universe},
    util::{Context, Result, android_build, download, downloads, replace_dir, which},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const DEBIAN: &str = "https://deb.debian.org/debian";
pub const SUITE: &str = "trixie";
/// Pinned x86-64 WoW64 Wine: 32-bit Windows guests need no AArch32 ABI.
const WINE: (&str, &str) = (
    "https://github.com/Kron4ek/Wine-Builds/releases/download/11.0/wine-11.0-amd64-wow64.tar.xz",
    "39574efa1132c3ca0d5c77dd2eddbe4a49cca0d6cc2c290ff4924493a1c40314",
);
pub const KOTLIN_VERSION: &str = "2.2.21";
const KOTLIN: (&str, &str) = (
    "https://github.com/JetBrains/kotlin/releases/download/v2.2.21/kotlin-compiler-2.2.21.zip",
    "a623871f1cd9c938946948b70ef9170879f0758043885bbd30c32f024e511714",
);

const USAGE: &str = "cargo xtask android <command>
  all          every step below that this host can run, then the APK
  kotlin       pinned Kotlin compiler (build/kotlin-2.2.21)
  wine         pinned Wine WoW64 build (build/wine)
  x64-libs     Debian x86-64 libstdc++/libgcc_s for Box64's Wine side
  box64        patched Box64 with BOX32, cross-compiled (build/box64)
  rootfs       Debian arm64 runtime image from packages (build/rootfs.tar.gz)
  resources    title adapters (banana-pepper-android-resources; Linux host)
  apk          the signed APK (build/BananaPepper.apk)
  input PORT FILE
               send transport input lines (m x y | b n 0|1 | k key 0|1 | @ms)
               through an `adb forward tcp:PORT localabstract:<socket>`";

pub fn main(args: &[String]) -> Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["all"] => all(),
        ["kotlin"] => kotlin().map(drop),
        ["wine"] => wine(),
        ["x64-libs"] => x64_libs(),
        ["box64"] => crate::box64::build(),
        ["rootfs"] => crate::rootfs::build(),
        ["resources"] if cfg!(target_os = "linux") => crate::dist::run(crate::dist::Host::Android, true),
        ["resources"] => Err("The title adapters link against i386 Linux libraries: build them on Linux (WSL works)".into()),
        ["apk"] => crate::apk::build(),
        ["input", port, file] => input(port, Path::new(file)),
        _ => Err(USAGE.into()),
    }
}

fn all() -> Result<()> {
    kotlin()?;
    wine()?;
    x64_libs()?;
    if cfg!(target_os = "linux") {
        crate::dist::run(crate::dist::Host::Android, true)?;
    } else {
        println!("Skipping the title adapters: they build on Linux (WSL works).");
    }
    crate::box64::build()?;
    crate::rootfs::build()?;
    crate::apk::build()
}

/// Android SDK, NDK and the newest installed build tools and platform.
pub struct Sdk {
    pub build_tools: PathBuf,
    pub platform: PathBuf,
    /// The NDK's prebuilt toolchain directory (its `bin` holds clang).
    pub prebuilt: PathBuf,
}

/// Newest entry of `dir` by numeric version components.
fn newest(dir: &Path) -> Result<PathBuf> {
    let key = |p: &PathBuf| -> Vec<u64> {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|s| s.parse().ok())
            .collect()
    };
    fs::read_dir(dir)
        .ctx(dir.display())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .max_by_key(key)
        .ok_or_else(|| format!("Nothing installed in {}", dir.display()))
}

impl Sdk {
    pub fn find() -> Result<Self> {
        let sdk = std::env::var_os("ANDROID_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                if cfg!(windows) {
                    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Android/Sdk"))
                } else {
                    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Android/Sdk"))
                }
            })
            .ok_or("Set ANDROID_HOME to the Android SDK")?;
        let ndk = match std::env::var_os("ANDROID_NDK_HOME") {
            Some(path) => PathBuf::from(path),
            None => newest(&sdk.join("ndk"))?,
        };
        let host = if cfg!(windows) {
            "windows-x86_64"
        } else if cfg!(target_os = "macos") {
            "darwin-x86_64"
        } else {
            "linux-x86_64"
        };
        let prebuilts = ndk.join("toolchains/llvm/prebuilt");
        let prebuilt = prebuilts.join(host);
        if !prebuilt.is_dir() {
            return Err(format!("The NDK has no {host} toolchain: install the NDK for this host"));
        }
        let platform = newest(&sdk.join("platforms"))?.join("android.jar");
        if !platform.is_file() {
            return Err(format!("Missing {}", platform.display()));
        }
        Ok(Self {
            build_tools: newest(&sdk.join("build-tools"))?,
            platform,
            prebuilt,
        })
    }

    /// The API-26 ARM64 clang driver of this host's NDK.
    pub fn clang(&self) -> PathBuf {
        let name = if cfg!(windows) {
            "aarch64-linux-android26-clang.cmd"
        } else {
            "aarch64-linux-android26-clang"
        };
        self.prebuilt.join("bin").join(name)
    }

    pub fn tool(&self, name: &str) -> PathBuf {
        let exe = if cfg!(windows) { ".exe" } else { "" };
        self.prebuilt.join("bin").join(format!("{name}{exe}"))
    }

    pub fn build_tool(&self, name: &str) -> PathBuf {
        let exe = if cfg!(windows) { ".exe" } else { "" };
        self.build_tools.join(format!("{name}{exe}"))
    }
}

/// The Kotlin compiler home (`…/kotlinc`).
pub fn kotlin() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("KOTLIN_HOME") {
        return Ok(PathBuf::from(home));
    }
    let home = android_build().join(format!("kotlin-{KOTLIN_VERSION}"));
    let kotlinc = home.join("kotlinc");
    if kotlinc.join("lib/kotlin-compiler.jar").is_file() {
        return Ok(kotlinc);
    }
    let archive = downloads()?.join(format!("kotlin-compiler-{KOTLIN_VERSION}.zip"));
    download(KOTLIN.0, &archive, KOTLIN.1)?;
    replace_dir(&home, |staging| {
        let file = fs::File::open(&archive).ctx(archive.display())?;
        zip::ZipArchive::new(file)
            .ctx("Kotlin archive")?
            .extract(staging)
            .ctx("Kotlin archive")
    })?;
    Ok(kotlinc)
}

/// Unpack the pinned Wine build into `build/wine` (once per checksum).
fn wine() -> Result<()> {
    let out = android_build().join("wine");
    let stamp = out.join(".banana-version");
    if fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == WINE.1) {
        return Ok(());
    }
    let archive = downloads()?.join("wine-11.0-amd64-wow64.tar.xz");
    download(WINE.0, &archive, WINE.1)?;
    println!("Unpacking {}", archive.display());
    let tar = deb::decompress(".xz", fs::read(&archive).ctx(archive.display())?)?;
    let mut tree = Tree::default();
    // Strip the archive's top-level directory.
    tree.merge_tar(&tar, &|p| p.split_once('/').map(|(_, rest)| rest.to_owned()))?;
    replace_dir(&out, |staging| {
        write_tree(&tree, staging)?;
        fs::write(staging.join(".banana-version"), format!("{}\n", WINE.1)).ctx("Wine stamp")
    })
}

/// Write a tree to disk: symlinks where the host supports them (copies of
/// their target otherwise), executable bits on Unix.
pub fn write_tree(tree: &Tree, root: &Path) -> Result<()> {
    if cfg!(unix) {
        for (name, entry) in &tree.entries {
            let target = root.join(name);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).ctx(parent.display())?;
            }
            match &entry.node {
                Node::Directory => fs::create_dir_all(&target).ctx(target.display())?,
                Node::File(bytes) => fs::write(&target, &bytes[..]).ctx(target.display())?,
                Node::Symlink(link) => {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(link, &target).ctx(target.display())?;
                    let _ = link;
                }
                Node::Hardlink(source) => {
                    fs::copy(root.join(source), &target).ctx(target.display())?;
                }
            }
            #[cfg(unix)]
            if !matches!(entry.node, Node::Symlink(_)) {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&target, fs::Permissions::from_mode(entry.mode | 0o200))
                    .ctx(target.display())?;
            }
        }
        Ok(())
    } else {
        tree.write_dir(root)
    }
}

/// Debian trixie amd64 libstdc++ and libgcc_s, matching the rootfs's C
/// library, with their copyright files.
fn x64_libs() -> Result<()> {
    let out = android_build().join("x64-libs");
    if out.join("libstdc++.so.6").is_file() && out.join("libgcc_s.so.1").is_file() {
        return Ok(());
    }
    let mut universe = Universe::new("amd64");
    universe.load(DEBIAN, &format!("dists/{SUITE}/main/binary-amd64/Packages.xz"))?;
    let cache = downloads()?;
    replace_dir(&out, |staging| {
        fs::create_dir_all(staging.join("licenses")).ctx("licenses")?;
        for (package, library) in [("libstdc++6", "libstdc++.so.6"), ("libgcc-s1", "libgcc_s.so.1")] {
            let index = universe
                .lookup(package, "amd64")
                .ok_or_else(|| format!("Debian has no {package}"))?;
            let mut tree = Tree::default();
            tree.merge_tar(&deb::data_tar(&universe.fetch(index, &cache)?)?, &deb::merged_usr)?;
            let path = tree
                .entries
                .keys()
                .find(|p| p.ends_with(&format!("/{library}")))
                .cloned()
                .ok_or_else(|| format!("{package} lacks {library}"))?;
            let bytes = tree
                .resolve(&path)
                .ok_or_else(|| format!("{package}: {library} is a dangling link"))?
                .to_vec();
            if bytes.get(..4) != Some(b"\x7fELF") || bytes.get(18..20) != Some(&[62, 0]) {
                return Err(format!("{library} is not x86-64"));
            }
            fs::write(staging.join(library), bytes).ctx(library)?;
            // Their doc folders link to the GCC base package's.
            let doc = format!("usr/share/doc/{package}");
            let copyright = match tree.entries.get(&doc).map(|e| &e.node) {
                Some(Node::Symlink(base)) => {
                    let index = universe
                        .lookup(base, "amd64")
                        .ok_or_else(|| format!("Debian has no {base}"))?;
                    let mut base_tree = Tree::default();
                    base_tree.merge_tar(&deb::data_tar(&universe.fetch(index, &cache)?)?, &deb::merged_usr)?;
                    base_tree.file(&format!("usr/share/doc/{base}/copyright")).map(<[u8]>::to_vec)
                }
                _ => tree.file(&format!("{doc}/copyright")).map(<[u8]>::to_vec),
            }
            .ok_or_else(|| format!("{package} has no copyright file"))?;
            fs::write(staging.join(format!("licenses/{package}.copyright")), copyright)
                .ctx("copyright")?;
        }
        Ok(())
    })
}

/// Replay input lines through an adb-forwarded transport socket.
fn input(port: &str, file: &Path) -> Result<()> {
    use std::io::Write;
    let port: u16 = port.parse().ctx("port")?;
    let text = fs::read_to_string(file).ctx(file.display())?;
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).ctx("adb forward")?;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(ms) = line.strip_prefix('@') {
            let ms: u64 = ms.parse().ctx(line)?;
            std::thread::sleep(std::time::Duration::from_millis(ms.min(10_000)));
        } else if ["m ", "b ", "k "].iter().any(|p| line.starts_with(p)) {
            stream.write_all(format!("{line}\n").as_bytes()).ctx("input socket")?;
        } else {
            return Err(format!("Invalid input command: {line}"));
        }
    }
    Ok(())
}

/// Locate `java` (JAVA_HOME first).
pub fn java() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let exe = if cfg!(windows) { "java.exe" } else { "java" };
        let path = PathBuf::from(home).join("bin").join(exe);
        if path.is_file() {
            return Ok(path);
        }
    }
    which("java").ok_or_else(|| "Install a JDK (java) or set JAVA_HOME".into())
}
