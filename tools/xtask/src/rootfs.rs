//! The Android runtime image: Debian trixie arm64 (plus the i386 libraries
//! Box32 loads) assembled straight from Debian's packages into
//! `build/rootfs.tar.gz`, the archive the app unpacks on first launch.
//!
//! Packages are read as archives and merged in memory, the way a minimal
//! debootstrap would lay them out, without running any package code. The few
//! maintainer-script results the runtime relies on are written directly.
use crate::{
    android::{DEBIAN, SUITE},
    deb::{self, Node, Tree, Universe},
    util::{Context, Result, android_build, downloads},
};
use std::{fs, sync::Arc};

/// What the runtime needs on top of Debian's essential set: Xvfb and
/// PulseAudio for the session, the native GL/X11/SDL/audio libraries Box64
/// and Box32 wrap, and the i386 libraries Box32 loads as x86 code.
const PACKAGES: &[&str] = &[
    "xvfb",
    "xauth",
    "x11-xserver-utils",
    "libgl1",
    "libglx-mesa0",
    "libgl1-mesa-dri",
    "libglu1-mesa",
    "libglut3.12",
    "libxcomposite1",
    "libxinerama1",
    "libxcursor1",
    "libxfixes3",
    "libxrandr2",
    "libxrender1",
    "libfreetype6",
    "libfontconfig1",
    "fontconfig-config",
    "fonts-dejavu-core",
    "libsdl1.2debian",
    "libpulse0",
    "pulseaudio",
    "libasound2-plugins",
    "libx11-6",
    "libxext6",
    "libxtst6",
    "libxi6",
    "libxxf86vm1",
    "libglut3.12:i386",
    "libgcc-s1:i386",
    "libstdc++6:i386",
    // Time zone data for guests' localtime (UTC, see `configure`).
    "tzdata",
];

/// Paths the runtime never uses. Box32 wraps the native arm64 GL/X11
/// libraries, so the i386 Mesa/LLVM pulled in as dependencies are dead weight.
fn dropped(path: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "usr/share/man/",
        "usr/share/info/",
        "usr/share/lintian/",
        "usr/share/locale/",
        "usr/share/i18n/",
        "usr/include/",
        "usr/lib/gcc/",
        "usr/libexec/gcc/",
        "var/lib/apt/lists/",
        "var/cache/",
        "tmp/",
        "usr/lib/i386-linux-gnu/dri/",
    ];
    const I386: &[&str] = &[
        "libLLVM", "libgallium", "libz3", "libicu", "libxml2", "libSPIRV", "libvulkan",
    ];
    if PREFIXES
        .iter()
        .any(|p| path.starts_with(p) || path == p.trim_end_matches('/'))
    {
        return true;
    }
    if let Some(name) = path.strip_prefix("usr/lib/i386-linux-gnu/") {
        return I386.iter().any(|p| name.starts_with(p));
    }
    false
}

/// Keep each package's copyright file (the image ships in the APK); drop
/// the rest of `/usr/share/doc`.
fn keep_doc(path: &str, node: &Node) -> bool {
    match path.strip_prefix("usr/share/doc/") {
        None => true,
        Some(rest) => match node {
            Node::Directory => !rest.contains('/'),
            Node::File(_) => rest.ends_with("/copyright") && rest.matches('/').count() == 1,
            _ => false,
        },
    }
}

pub fn build() -> Result<()> {
    let build = android_build();
    let box64 = build.join("box64");
    let install = build.join("box64-install");
    if !box64.is_file() || !install.is_dir() {
        return Err("Build Box64 first: cargo xtask android box64".into());
    }
    let mut universe = Universe::new("arm64");
    for arch in ["arm64", "i386"] {
        universe.load(DEBIAN, &format!("dists/{SUITE}/main/binary-{arch}/Packages.xz"))?;
    }
    let mut roots = universe.essential();
    roots.extend(PACKAGES.iter().map(|p| p.to_string()));
    let selected = universe.resolve(&roots)?;
    println!("Assembling {} Debian packages", selected.len());
    let mut manifest: Vec<String> = selected
        .iter()
        .map(|&i| {
            let p = &universe.packages[i];
            format!("{} {} {}", p.name, p.arch, p.version)
        })
        .collect();
    manifest.sort();
    fs::write(build.join("rootfs.packages"), manifest.join("\n") + "\n").ctx("package list")?;
    let cache = downloads()?;
    let mut tree = Tree::default();
    let map = |p: &str| deb::merged_usr(p).filter(|p| !dropped(p));
    for (n, &index) in selected.iter().enumerate() {
        let package = &universe.packages[index];
        if n % 25 == 0 {
            println!("  [{n}/{}] {} {}", selected.len(), package.name, package.version);
        }
        tree.merge_tar(&deb::data_tar(&universe.fetch(index, &cache)?)?, &map)?;
    }
    tree.entries.retain(|path, entry| keep_doc(path, &entry.node));
    configure(&mut tree)?;
    add_box64(&mut tree, &fs::read(&box64).ctx(box64.display())?, &install)?;
    tree.add_parents();
    let out = build.join("rootfs.tar.gz");
    println!("Writing {}", out.display());
    tree.write_tar_gz(&out)?;
    println!(
        "Runtime image ready: {} ({} MB)",
        out.display(),
        fs::metadata(&out).ctx("rootfs")?.len() >> 20
    );
    Ok(())
}

/// The maintainer-script results the runtime relies on.
fn configure(tree: &mut Tree) -> Result<()> {
    let file = |bytes: &[u8]| Node::File(Arc::new(bytes.to_vec()));
    // Merged /usr aliases.
    for alias in ["bin", "sbin", "lib"] {
        tree.insert(alias, Node::Symlink(format!("usr/{alias}")), 0o777);
    }
    // base-passwd's update-passwd: the account databases from its masters.
    for name in ["passwd", "group"] {
        let master = tree
            .file(&format!("usr/share/base-passwd/{name}.master"))
            .ok_or_else(|| format!("base-passwd lacks {name}.master"))?
            .to_vec();
        tree.insert(&format!("etc/{name}"), file(&master), 0o644);
    }
    // base-files' postinst: its templates for /etc and root's home.
    for (template, target) in [
        ("profile", "etc/profile"),
        ("motd", "etc/motd"),
        ("dot.profile", "root/.profile"),
        ("dot.bashrc", "root/.bashrc"),
    ] {
        if let Some(bytes) = tree.file(&format!("usr/share/base-files/{template}")).map(<[u8]>::to_vec) {
            tree.insert(target, file(&bytes), 0o644);
        }
    }
    // libc-bin's postinst: the name service configuration.
    if let Some(bytes) = tree.file("usr/share/libc-bin/nsswitch.conf").map(<[u8]>::to_vec) {
        tree.insert("etc/nsswitch.conf", file(&bytes), 0o644);
    }
    // update-alternatives for the tools scripts call by their generic names.
    for (name, target) in [("awk", "mawk"), ("nawk", "mawk"), ("which", "which.debianutils")] {
        if tree.entries.contains_key(&format!("usr/bin/{target}")) {
            tree.insert(&format!("usr/bin/{name}"), Node::Symlink(target.into()), 0o777);
        }
    }
    // fontconfig-config's postinst: Debian's default rendering choices.
    for conf in ["10-hinting-slight.conf", "70-no-bitmaps-except-emoji.conf"] {
        let available = format!("usr/share/fontconfig/conf.avail/{conf}");
        if tree.entries.contains_key(&available) {
            tree.insert(&format!("etc/fonts/conf.d/{conf}"), Node::Symlink(format!("/{available}")), 0o777);
        }
    }
    // tzdata's postinst: the system zone.
    tree.insert("etc/localtime", Node::Symlink("/usr/share/zoneinfo/Etc/UTC".into()), 0o777);
    tree.insert("etc/timezone", file(b"Etc/UTC\n"), 0o644);
    // dash's postinst: /bin/sh.
    if !tree.entries.contains_key("usr/bin/sh") {
        tree.insert("usr/bin/sh", Node::Symlink("dash".into()), 0o777);
    }
    tree.insert(
        "etc/hosts",
        file(b"127.0.0.1 localhost localhost.localdomain\n::1 localhost ip6-localhost ip6-loopback\n"),
        0o644,
    );
    for (path, mode) in [
        ("tmp", 0o1777),
        ("proc", 0o555),
        ("sys", 0o555),
        ("dev", 0o755),
        ("run", 0o755),
        ("root", 0o700),
        ("opt/arcade", 0o755),
    ] {
        tree.insert(path, Node::Directory, mode);
    }
    for required in ["usr/bin/Xvfb", "usr/bin/xkbcomp", "usr/bin/pulseaudio", "usr/bin/env", "usr/bin/true"] {
        if !tree.entries.contains_key(required) {
            return Err(format!("The assembled runtime lacks {required}"));
        }
    }
    Ok(())
}

fn add_box64(tree: &mut Tree, binary: &[u8], install: &std::path::Path) -> Result<()> {
    tree.insert("usr/local/bin/box64", Node::File(Arc::new(binary.to_vec())), 0o755);
    let mut pending = vec![install.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).ctx(dir.display())? {
            let path = entry.ctx("box64 install")?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(install)
                .ctx("box64 install")?
                .to_string_lossy()
                .replace('\\', "/");
            if relative.starts_with('.') {
                continue;
            }
            let bytes = fs::read(&path).ctx(path.display())?;
            tree.insert(&relative, Node::File(Arc::new(bytes)), 0o644);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimming_keeps_copyrights_and_native_graphics() {
        assert!(dropped("usr/share/man/man1/ls.1.gz"));
        assert!(dropped("usr/lib/i386-linux-gnu/libLLVM.so.19.1"));
        assert!(!dropped("usr/lib/aarch64-linux-gnu/libLLVM.so.19.1"));
        assert!(!dropped("usr/lib/i386-linux-gnu/libglut.so.3.12"));
        let file = Node::File(Arc::new(Vec::new()));
        assert!(keep_doc("usr/share/doc/libc6/copyright", &file));
        assert!(!keep_doc("usr/share/doc/libc6/changelog.gz", &file));
        assert!(!keep_doc("usr/share/doc/libfoo", &Node::Symlink("libbar".into())));
        assert!(keep_doc("usr/bin/env", &file));
    }
}
