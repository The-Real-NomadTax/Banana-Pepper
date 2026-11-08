//! `cargo xtask android …`: every input of the APK, then the APK itself.
use crate::{
    deb::{self, Node, Tree, Universe},
    util::{Context, Result, android_build, download, downloads, replace_dir, root, run, which},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const DEBIAN: &str = "https://deb.debian.org/debian";
pub const SUITE: &str = "trixie";
const TERMUX: &str = "https://packages.termux.dev/apt/termux-main";
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
const PROOT_COMMIT: &str = "52d65a4dc140ca554d241a1438ddd4f6b00c9813";
pub const PROOT_PROTOCOL: &[u8] = b"BANANA_VFS_PROTOCOL_2";

const USAGE: &str = "cargo xtask android <command>
  all          every step below that this host can run, then the APK
  kotlin       pinned Kotlin compiler (build/kotlin-2.2.21)
  wine         pinned Wine WoW64 build (build/wine)
  x64-libs     Debian x86-64 libstdc++/libgcc_s for Box64's Wine side
  proot-deps   Termux talloc/libandroid-shmem and headers (build/proot)
  proot        patched PRoot tracer (Linux host)
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
        ["proot-deps"] => proot_deps(),
        ["proot"] => proot(),
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
    proot_deps()?;
    if cfg!(target_os = "linux") {
        proot()?;
        crate::dist::run(crate::dist::Host::Android, true)?;
    } else {
        println!("Skipping the PRoot tracer and title adapters: they build on Linux (WSL works).");
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
    /// Whether that toolchain is built for this host. A Windows NDK seen
    /// from WSL is not: its sysroot is usable, its compilers are not.
    native: bool,
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
        let native = prebuilts.join(host).is_dir();
        let prebuilt = if native {
            prebuilts.join(host)
        } else {
            newest(&prebuilts)?
        };
        let platform = newest(&sdk.join("platforms"))?.join("android.jar");
        if !platform.is_file() {
            return Err(format!("Missing {}", platform.display()));
        }
        Ok(Self {
            build_tools: newest(&sdk.join("build-tools"))?,
            platform,
            prebuilt,
            native,
        })
    }

    /// Whether the NDK's own compilers run on this host.
    pub fn native_toolchain(&self) -> bool {
        self.native && self.clang().is_file()
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

/// Termux's proot, talloc and libandroid-shmem: the libraries and headers the
/// patched tracer links against, and the stock loader.
fn proot_deps() -> Result<()> {
    let out = android_build().join("proot");
    if out.join("include/talloc.h").is_file() && out.join("libtalloc.so.2").is_file() {
        return Ok(());
    }
    let mut universe = Universe::new("aarch64");
    universe.load(TERMUX, "dists/stable/main/binary-aarch64/Packages")?;
    let cache = downloads()?;
    let mut tree = Tree::default();
    for package in ["proot", "libtalloc", "libandroid-shmem"] {
        let index = universe
            .lookup(package, "aarch64")
            .ok_or_else(|| format!("Termux has no {package}"))?;
        println!("Termux {package} {}", universe.packages[index].version);
        let prefix = "data/data/com.termux/files/usr/";
        tree.merge_tar(&deb::data_tar(&universe.fetch(index, &cache)?)?, &|p| {
            p.strip_prefix(prefix).map(str::to_owned)
        })?;
    }
    fs::create_dir_all(out.join("include")).ctx("proot include")?;
    let pick = |pattern: &dyn Fn(&str) -> bool| -> Result<Vec<u8>> {
        let path = tree
            .entries
            .iter()
            .find(|(p, e)| pattern(p) && matches!(e.node, Node::File(_)))
            .map(|(p, _)| p.clone())
            .ok_or("Termux packages lack an expected file")?;
        Ok(tree.file(&path).unwrap().to_vec())
    };
    for (name, bytes) in [
        ("loader", pick(&|p| p == "libexec/proot/loader")?),
        ("libtalloc.so.2", pick(&|p| p.starts_with("lib/libtalloc.so.2."))?),
        ("libandroid-shmem.so", pick(&|p| p == "lib/libandroid-shmem.so")?),
    ] {
        fs::write(out.join(name), bytes).ctx(name)?;
    }
    for (path, entry) in &tree.entries {
        if let (Some(header), Node::File(bytes)) = (path.strip_prefix("include/"), &entry.node) {
            let target = out.join("include").join(header);
            fs::create_dir_all(target.parent().unwrap()).ctx("include")?;
            fs::write(&target, &bytes[..]).ctx(target.display())?;
        }
    }
    Ok(())
}

/// Apply the view extension to a PRoot checkout (idempotent).
fn patch_proot(source: &Path) -> Result<()> {
    let extension = root().join("backends/android/proot");
    for file in ["banana_vfs.c", "banana_fd.h"] {
        fs::copy(extension.join(file), source.join("src/extension").join(file)).ctx(file)?;
    }
    let edit = |path: &str, marker: &str, change: &dyn Fn(String) -> String| -> Result<()> {
        let path = source.join(path);
        let text = fs::read_to_string(&path).ctx(path.display())?;
        if !text.contains(marker) {
            let changed = change(text.clone());
            if changed == text {
                return Err(format!("{} no longer matches the PRoot patch", path.display()));
            }
            fs::write(&path, changed).ctx(path.display())?;
        }
        Ok(())
    };
    edit("src/GNUmakefile", "extension/banana_vfs.o", &|s| {
        s.replacen("OBJECTS += \\\n", "OBJECTS += extension/banana_vfs.o\n\nOBJECTS += \\\n", 1)
    })?;
    let first = "\t/* Pre-configure the first tracee.  */";
    edit("src/cli/cli.c", "banana_vfs_callback", &|s| {
        s.replacen(first, &format!("\t/* Initialize before parsing -w: its directory may be virtual. */
    if (getenv(\"BANANA_VFS_SOCKET\") != NULL) {{
        extern int banana_vfs_callback(Extension *, ExtensionEvent, intptr_t, intptr_t);
        if (initialize_extension(tracee, banana_vfs_callback, NULL) < 0) goto error;
    }}
{first}"), 1)
    })?;
    edit("src/cli/cli.c", "banana_vfs_guest_path", &|s| {
        s.replacen(
            "status = canonicalize(tracee, path2, true, path, 0);",
            "extern int banana_vfs_guest_path(Tracee *, char *, const char *, int);
    status = banana_vfs_guest_path(tracee, path, path2, 1);
    if (status == 0) status = canonicalize(tracee, path2, true, path, 0);
    else if (status > 0) status = 0;",
            1,
        )
    })?;
    edit("src/path/path.c", "banana_vfs_guest_path", &|s| {
        s.replacen(
            "return canonicalize(tracee, joined, deref_final, guest_path, 0);",
            "extern int banana_vfs_guest_path(Tracee *, char *, const char *, int);
    status = banana_vfs_guest_path(tracee, guest_path, joined, 0);
    if (status != 0) return status > 0 ? 0 : status;
    return canonicalize(tracee, joined, deref_final, guest_path, 0);",
            1,
        )
    })?;
    // New NDK clang versions require the declarations this unit uses.
    edit("src/extension/ashmem_memfd/ashmem_memfd.c", "#include <string.h>", &|s| {
        format!("#include <string.h>\n{s}")
    })?;
    // Sessions own every traced descendant, including Wine servers in other
    // process groups: the kernel reaps them if their tracer dies.
    edit("src/tracee/event.c", "PTRACE_O_EXITKILL", &|s| {
        s.replace("PTRACE_O_TRACEEXIT);", "PTRACE_O_TRACEEXIT | PTRACE_O_EXITKILL);")
    })
}

/// Check out pinned PRoot, apply the view extension and build it with the
/// NDK. Needs a Linux host for PRoot's makefiles; an NDK built for another
/// host (the Windows NDK seen from WSL) is used through the host's clang.
fn proot() -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Err("The PRoot tracer builds on a Linux host (WSL works)".into());
    }
    proot_deps()?;
    let sdk = Sdk::find()?;
    let out = android_build().join("proot");
    let source = android_build().join("proot-src");
    if !source.join(".git").is_dir() {
        run(Command::new("git").args(["clone", "https://github.com/termux/proot.git"]).arg(&source))?;
    }
    run(Command::new("git").arg("-C").arg(&source).args(["fetch", "origin", PROOT_COMMIT]))?;
    run(Command::new("git")
        .arg("-C")
        .arg(&source)
        .args(["checkout", "--force", "--detach", PROOT_COMMIT]))?;
    // Objects and dependency files from another toolchain must not leak in.
    run(Command::new("git").arg("-C").arg(&source).args(["clean", "-fdxq"]))?;
    patch_proot(&source)?;
    let quote = |p: &Path| format!("\"{}\"", p.display());
    let (cc, strip, objcopy, objdump) = if sdk.native_toolchain() {
        (
            sdk.clang().display().to_string(),
            sdk.tool("llvm-strip"),
            sdk.tool("llvm-objcopy"),
            sdk.tool("llvm-objdump"),
        )
    } else {
        let resource = newest(&sdk.prebuilt.join("lib/clang"))?;
        let lld = std::env::var("LLD_DIR").map(|d| format!(" -B{d}")).unwrap_or_default();
        (
            format!(
                "clang --target=aarch64-linux-android26 --sysroot={} -resource-dir={} --rtlib=compiler-rt --unwindlib=libunwind -fuse-ld=lld{lld}",
                quote(&sdk.prebuilt.join("sysroot")),
                quote(&resource)
            ),
            PathBuf::from("aarch64-linux-gnu-strip"),
            PathBuf::from("aarch64-linux-gnu-objcopy"),
            PathBuf::from("aarch64-linux-gnu-objdump"),
        )
    };
    let jobs = std::thread::available_parallelism().map_or(4, usize::from);
    run(Command::new("make")
        .arg("-C")
        .arg(source.join("src"))
        .arg(format!("-j{jobs}"))
        .arg(format!("CC={cc}"))
        .arg(format!(
            "CPPFLAGS=-D_FILE_OFFSET_BITS=64 -D_GNU_SOURCE -I. -I{} -I{}",
            quote(&source.join("src")),
            quote(&out.join("include"))
        ))
        .arg(format!(
            "LDFLAGS=-L{} -l:libtalloc.so.2 -landroid-shmem -Wl,-z,noexecstack -Wl,-z,max-page-size=16384",
            quote(&out)
        ))
        .arg("PROOT_WITH_LIBANDROID_SHMEM=1")
        .arg("V=1")
        .arg(format!("STRIP={}", strip.display()))
        .arg(format!("OBJCOPY={}", objcopy.display()))
        .arg(format!("OBJDUMP={}", objdump.display())))?;
    fs::copy(source.join("src/proot"), out.join("proot")).ctx("proot")?;
    fs::copy(source.join("src/loader/loader"), out.join("loader")).ctx("loader")?;
    run(Command::new(&strip).arg("--strip-unneeded").arg(out.join("proot")))
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
