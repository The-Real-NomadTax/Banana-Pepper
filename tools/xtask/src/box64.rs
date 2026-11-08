//! Box64 with BOX32 and the Banana Pepper patches, cross-compiled for
//! Debian arm64 with clang and lld against a sysroot made from Debian's own
//! packages: no phone, emulator or arm64 container is involved.
use crate::{
    android::{DEBIAN, SUITE},
    deb::{self, Tree, Universe},
    util::{Context, Result, android_build, downloads, root, run, which},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const REPOSITORY: &str = "https://github.com/ptitSeb/box64";
const COMMIT: &str = "59b3eb1555406db697941ddd5e414c7085b90e4f";
const PATCHES: &[&str] = &[
    "box64-sectionless-elf32.patch",
    "box64-android-guest-exec.patch",
    "box32-pthread-attribute-width.patch",
];
/// What the link step needs: C runtime objects, libc, libgcc and headers.
const SYSROOT: &[&str] = &[
    "libc6",
    "libc6-dev",
    "linux-libc-dev",
    "libcrypt1",
    "libcrypt-dev",
    "libgcc-s1",
    "libgcc-14-dev",
];

/// Outputs: `build/box64` (the binary) and `build/box64-install` (the files
/// Box64 installs next to it: its rc file and BOX32's i386 libraries).
pub fn build() -> Result<()> {
    let build = android_build();
    let source = build.join("box64-src");
    let stamp = build.join("box64-install/.banana-version");
    let version = format!("{COMMIT} {}", patches_digest()?);
    if build.join("box64").is_file() && fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == version) {
        return Ok(());
    }
    checkout(&source)?;
    let sysroot = sysroot()?;
    let binary = compile(&source, &sysroot)?;
    let install = build.join("box64-install");
    crate::util::replace_dir(&install, |staging| {
        fs::create_dir_all(staging.join("etc")).ctx("etc")?;
        fs::copy(source.join("system/box64.box64rc"), staging.join("etc/box64.box64rc")).ctx("box64rc")?;
        let libraries = staging.join("usr/lib/box64-i386-linux-gnu");
        fs::create_dir_all(&libraries).ctx("box32 libraries")?;
        for name in [
            "libstdc++.so.5",
            "libstdc++.so.6",
            "libgcc_s.so.1",
            "libpng12.so.0",
            "libunwind.so.8",
            "libcrypto.so.1.0.0",
            "libssl.so.1.0.0",
            "libz.so.1",
        ] {
            fs::copy(source.join("x86lib").join(name), libraries.join(name)).ctx(name)?;
        }
        fs::write(staging.join(".banana-version"), format!("{version}\n")).ctx("stamp")
    })?;
    fs::copy(&binary, build.join("box64")).ctx("box64")?;
    println!("Box64 ready: {}", build.join("box64").display());
    Ok(())
}

fn patches() -> PathBuf {
    root().join("apps/android/patches")
}

fn patches_digest() -> Result<String> {
    let mut all = Vec::new();
    for patch in PATCHES {
        all.extend(fs::read(patches().join(patch)).ctx(patch)?);
    }
    Ok(crate::util::sha256(&all)[..16].to_owned())
}

fn checkout(source: &Path) -> Result<()> {
    let git = |args: &[&str]| run(Command::new("git").arg("-C").arg(source).args(args));
    if !source.join(".git").is_dir() {
        run(Command::new("git").args(["clone", REPOSITORY]).arg(source))?;
    }
    git(&["fetch", "origin", COMMIT])?;
    git(&["checkout", "--force", "--detach", COMMIT])?;
    git(&["clean", "-fdq"])?;
    for patch in PATCHES {
        run(Command::new("git")
            .arg("-C")
            .arg(source)
            .args(["apply", "--whitespace=nowarn"])
            .arg(patches().join(patch)))?;
    }
    Ok(())
}

/// A Debian arm64 sysroot for linking, rebuilt when missing.
fn sysroot() -> Result<PathBuf> {
    let path = android_build().join("box64-sysroot");
    if path.join("usr/include/stdio.h").is_file() {
        return Ok(path);
    }
    let mut universe = Universe::new("arm64");
    universe.load(DEBIAN, &format!("dists/{SUITE}/main/binary-arm64/Packages.xz"))?;
    let cache = downloads()?;
    let mut tree = Tree::default();
    for package in SYSROOT {
        let index = universe
            .lookup(package, "arm64")
            .or_else(|| universe.lookup(package, "all"))
            .ok_or_else(|| format!("Debian arm64 has no {package}"))?;
        tree.merge_tar(&deb::data_tar(&universe.fetch(index, &cache)?)?, &deb::merged_usr)?;
    }
    // glibc's linker scripts name /lib paths: mirror the merged-/usr `lib`
    // alias as real entries, since a sysroot may live where symlinks cannot.
    let aliased: Vec<_> = tree
        .entries
        .iter()
        .filter_map(|(p, e)| Some((format!("lib/{}", p.strip_prefix("usr/lib/")?), e.clone())))
        .collect();
    tree.entries.extend(aliased);
    crate::util::replace_dir(&path, |staging| tree.write_dir(staging))?;
    Ok(path)
}

/// A CMake and Ninja: the Android SDK's native copies first (a PATH CMake
/// may be an MSYS build that rewrites Windows paths), then PATH.
fn build_tools() -> Result<(PathBuf, PathBuf)> {
    let sdk_cmake = || -> Option<PathBuf> {
        let sdk = crate::android::Sdk::find().ok()?;
        let dir = sdk.build_tools.parent()?.parent()?.join("cmake");
        let newest = fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).max()?;
        Some(newest.join("bin"))
    };
    let exe = if cfg!(windows) { ".exe" } else { "" };
    let find = |name: &str| {
        sdk_cmake()
            .map(|d| d.join(format!("{name}{exe}")))
            .filter(|p| p.is_file())
            .or_else(|| which(name))
    };
    Ok((
        find("cmake").ok_or("Install CMake (or the Android SDK's cmake package)")?,
        find("ninja").ok_or("Install Ninja (or the Android SDK's cmake package)")?,
    ))
}

fn compile(source: &Path, sysroot: &Path) -> Result<PathBuf> {
    let (cmake, ninja) = build_tools()?;
    let clang = which("clang").ok_or("Install LLVM clang and lld")?;
    let strip = which("llvm-strip").ok_or("Install LLVM (llvm-strip)")?;
    let out = android_build().join("box64-build");
    let path = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let link = "-fuse-ld=lld";
    run(Command::new(&cmake)
        .arg("-S")
        .arg(source)
        .arg("-B")
        .arg(&out)
        .args(["-G", "Ninja"])
        .arg(format!("-DCMAKE_MAKE_PROGRAM={}", path(&ninja)))
        .args([
            "-DCMAKE_SYSTEM_NAME=Linux",
            "-DCMAKE_SYSTEM_PROCESSOR=aarch64",
            "-DCMAKE_C_COMPILER_TARGET=aarch64-linux-gnu",
            "-DCMAKE_CXX_COMPILER_TARGET=aarch64-linux-gnu",
            "-DCMAKE_ASM_COMPILER_TARGET=aarch64-linux-gnu",
            "-DCMAKE_BUILD_TYPE=RelWithDebInfo",
            "-DARM64=ON",
            "-DARM_DYNAREC=ON",
            "-DBOX32=ON",
            "-DBOX32_BINFMT=OFF",
            "-DNOGIT=ON",
            // The generated wrappers are committed upstream and the patches
            // leave wrapper definitions alone: skip regenerating them.
            "-DCI=ON",
        ])
        .arg(format!("-DCMAKE_C_COMPILER={}", path(&clang)))
        .arg(format!("-DCMAKE_ASM_COMPILER={}", path(&clang)))
        .arg(format!("-DCMAKE_SYSROOT={}", path(sysroot)))
        .arg(format!("-DCMAKE_EXE_LINKER_FLAGS={link}"))
        .arg(format!("-DCMAKE_SHARED_LINKER_FLAGS={link}")))?;
    let jobs = std::thread::available_parallelism().map_or(4, usize::from);
    run(Command::new(&cmake)
        .arg("--build")
        .arg(&out)
        .args(["--target", "box64", "-j", &jobs.to_string()]))?;
    let binary = out.join("box64");
    let stripped = out.join("box64.stripped");
    run(Command::new(strip)
        .arg("--strip-debug")
        .arg(&binary)
        .arg("-o")
        .arg(&stripped))?;
    let bytes = fs::read(&stripped).ctx("box64")?;
    if bytes.get(..4) != Some(b"\x7fELF") || bytes.get(18..20) != Some(&[183, 0]) {
        return Err("The Box64 build is not an AArch64 ELF".into());
    }
    Ok(stripped)
}
