//! Build/package only crate-declared artifacts for the current host, or the
//! Android resource package (title adapters) for the APK.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Debug)]
struct Artifact {
    package: String,
    name: String,
    kind: String,
    target: String,
    destination: String,
}
fn declarations(metadata: &Value, host: &str) -> Result<Vec<Artifact>, String> {
    let mut output = Vec::new();
    let mut destinations = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    for p in metadata["packages"]
        .as_array()
        .ok_or("Missing workspace packages")?
    {
        if let Some(records) = p
            .pointer("/metadata/banana/artifacts")
            .and_then(Value::as_array)
        {
            for a in records {
                if !a["hosts"]
                    .as_array()
                    .ok_or("Missing artifact hosts")?
                    .iter()
                    .any(|h| h == host)
                {
                    continue;
                }
                let get = |k: &str| {
                    a[k].as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| format!("Missing artifact {k}"))
                };
                let destination = get("destination")?
                    .replace("{exe}", if host == "windows" { ".exe" } else { "" });
                banana_content::path(&destination).map_err(|e| e.to_string())?;
                if !destinations.insert(destination.clone()) {
                    return Err(format!("Duplicate artifact destination {destination}"));
                }
                let name = get("name")?;
                if !names.insert(name.clone()) {
                    return Err(format!("Duplicate artifact name {name}"));
                }
                output.push(Artifact {
                    package: p["name"].as_str().ok_or("Missing package name")?.into(),
                    name,
                    kind: get("kind")?,
                    target: get("target")?,
                    destination,
                });
            }
        }
    }
    Ok(output)
}
fn command(c: &mut Command) -> Result<(), String> {
    let s = c.status().map_err(|e| e.to_string())?;
    if s.success() {
        Ok(())
    } else {
        Err(format!("Build command failed with {s}"))
    }
}
fn output(target: &Path, a: &Artifact) -> PathBuf {
    let mut p = target.to_path_buf();
    if !a.target.is_empty() {
        p = p.join(&a.target);
    }
    let windows = if a.target.is_empty() {
        cfg!(windows)
    } else {
        a.target.contains("windows")
    };
    let name = if a.kind == "bin" {
        format!("{}{}", a.name, if windows { ".exe" } else { "" })
    } else if windows {
        format!("{}.dll", a.name)
    } else {
        format!("lib{}.so", a.name)
    };
    p.join("release").join(name)
}
/// Which package to assemble.
#[derive(Clone, Copy, PartialEq)]
pub enum Host {
    /// The desktop distribution of the running OS.
    Current,
    /// `banana-pepper-android-resources`: the APK's title adapters.
    Android,
}

/// Assemble the package, first building its artifacts when `build` is set.
pub fn run(target_host: Host, build: bool) -> Result<(), String> {
    let root = crate::util::root();
    let android = target_host == Host::Android;
    let host = if android {
        "android"
    } else {
        std::env::consts::OS
    };
    let metadata = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1", "--locked"])
        .current_dir(&root)
        .output()
        .map_err(|e| e.to_string())?;
    if !metadata.status.success() {
        return Err(String::from_utf8_lossy(&metadata.stderr).into());
    }
    let metadata: Value = serde_json::from_slice(&metadata.stdout).map_err(|e| e.to_string())?;
    let artifacts = declarations(&metadata, host)?;
    let artifact_target = PathBuf::from(
        metadata["target_directory"]
            .as_str()
            .ok_or("Cargo metadata has no target directory")?,
    );
    if build {
        let mut targets = std::collections::BTreeSet::new();
        for a in &artifacts {
            if !a.target.is_empty() {
                targets.insert(a.target.clone());
            }
        }
        for t in targets {
            command(Command::new("rustup").args(["target", "add", &t]))?;
        }
        for a in &artifacts {
            let mut c = Command::new("cargo");
            c.current_dir(&root)
                .args(["build", "--release", "--locked", "-p", &a.package]);
            if a.kind == "bin" {
                c.args(["--bin", &a.name]);
            } else {
                c.arg("--lib");
            }
            if !a.target.is_empty() {
                c.args(["--target", &a.target]);
            }
            command(&mut c)?;
        }
    }
    let name = if android {
        "banana-pepper-android-resources".to_owned()
    } else {
        format!(
            "banana-pepper-{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    };
    let base = root.join("target/dist");
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let staging = base.join(format!(".{name}-staging"));
    if staging.exists() {
        remove_owned(&root, &staging)?;
    }
    fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let mut catalog: BTreeMap<String, String> = fs::read(root.join("target/artifacts.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    for a in &artifacts {
        let source = output(&artifact_target, a);
        let bytes = fs::read(&source).map_err(|e| {
            format!(
                "Missing artifact {}: {e}. Run cargo xtask dist.",
                source.display()
            )
        })?;
        write(&staging, &a.destination, &bytes)?;
        fs::set_permissions(
            staging.join(&a.destination),
            fs::metadata(&source)
                .map_err(|e| e.to_string())?
                .permissions(),
        )
        .map_err(|e| e.to_string())?;
        catalog.insert(a.name.clone(), source.to_string_lossy().into_owned());
    }
    // Plans name artifacts; the package maps each name to its location.
    write(
        &staging,
        "artifacts.json",
        &serde_json::to_vec_pretty(
            &artifacts
                .iter()
                .map(|a| (a.name.clone(), a.destination.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
        .map_err(|e| e.to_string())?,
    )?;
    banana_compat::install();
    if !android {
        write(
            &staging,
            "settings.cfg",
            banana_settings::Settings::default().to_text().as_bytes(),
        )?;
        write(
            &staging,
            "controls.cfg",
            banana_input::controls::Bindings::default()
                .to_text()
                .as_bytes(),
        )?;
    }
    write(
        &staging,
        banana_runtime::DIST_MARKER,
        b"banana-pepper distribution v2\n",
    )?;
    write(&staging, "README.txt", include_bytes!("../README-dist.txt"))?;
    // Shared docs, then each game's own docs from game-compat/<game>/docs.
    let mut docs = vec![(root.join("docs"), "docs".to_owned())];
    for e in fs::read_dir(root.join("game-compat")).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        if e.path().join("docs").is_dir() {
            docs.push((
                e.path().join("docs"),
                format!("docs/games/{}", e.file_name().to_string_lossy()),
            ));
        }
    }
    for (directory, destination) in docs {
        for e in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            if e.path().extension().is_some_and(|e| e == "md") {
                write(
                    &staging,
                    &format!("{destination}/{}", e.file_name().to_string_lossy()),
                    &fs::read(e.path()).map_err(|e| e.to_string())?,
                )?;
            }
        }
    }
    write(&staging,"THIRD-PARTY-NOTICES.txt",b"Box64: MIT; https://github.com/ptitSeb/box64\nFUSE/fuser: MIT; https://github.com/cberner/fuser\nZArchive/zarchive-rs: MIT; see their source distributions.\niced-x86: MIT; https://github.com/icedland/iced\nx11rb (Android display bridge): MIT OR Apache-2.0; https://github.com/psychon/x11rb\nAndroid packages also carry: Wine (LGPL-2.1-or-later; https://gitlab.winehq.org/wine/wine, binaries from https://github.com/Kron4ek/Wine-Builds); a Debian trixie arm64 runtime image (per-package licenses in its /usr/share/doc/*/copyright; sources at https://sources.debian.org); the Debian x86-64 GCC runtime libraries libstdc++ and libgcc_s (GPL-3.0-or-later with the GCC Runtime Library Exception; x64-libs/licenses); and the Kotlin standard library (Apache-2.0).\nLinux requires /dev/fuse and unprivileged user namespaces; Wine for PE games.\n")?;
    let inventory = inventory(&staging)?;
    write(&staging,"manifest.json",&serde_json::to_vec_pretty(&json!({"version":2,"files":inventory,"os":host,"arch":if android {"aarch64"}else{std::env::consts::ARCH}})).map_err(|e|e.to_string())?)?;
    fs::write(
        root.join("target/artifacts.json"),
        serde_json::to_vec_pretty(&catalog).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let destination = base.join(&name);
    let previous = base.join(format!(".{name}-previous"));
    check_output_path(&root, &staging)?;
    check_output_path(&root, &destination)?;
    check_output_path(&root, &previous)?;
    if previous.exists() {
        if !banana_runtime::is_distribution(&previous) {
            return Err("Refusing to remove an unrecognized previous distribution".into());
        }
        remove_owned(&root, &previous)?;
    }
    if destination.exists() {
        if !banana_runtime::is_distribution(&destination) {
            return Err("Refusing to replace a directory without the distribution marker".into());
        }
        fs::rename(&destination, &previous).map_err(|e| {
            format!("Cannot replace the previous package; close running package applications: {e}")
        })?;
    }
    if let Err(error) = fs::rename(&staging, &destination) {
        if previous.exists() {
            check_output_path(&root, &previous)?;
            check_output_path(&root, &destination)?;
            let _ = fs::rename(&previous, &destination);
        }
        return Err(error.to_string());
    }
    if previous.exists() {
        remove_owned(&root, &previous)?;
    }
    println!("Distribution ready: {}", destination.display());
    Ok(())
}
fn write(root: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let p = root.join(name);
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(p, bytes).map_err(|e| e.to_string())
}
fn remove_owned(root: &Path, path: &Path) -> Result<(), String> {
    let target = path.canonicalize().map_err(|e| e.to_string())?;
    let intended = root
        .join("target/dist")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !target.starts_with(&intended) || target == intended {
        return Err("Refusing deletion outside the distribution output".into());
    }
    fs::remove_dir_all(target).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_filters_hosts_and_rejects_unsafe_or_duplicate_destinations() {
        let package = |destination: &str| json!({"name":"future-adapter","metadata":{"banana":{"artifacts":[{"name":"adapter","kind":"lib","target":"i686-pc-windows-msvc","hosts":["windows"],"destination":destination}]}}});
        assert_eq!(
            declarations(
                &json!({"packages":[package("games/future/adapter.dll")]}),
                "windows"
            )
            .unwrap()
            .len(),
            1
        );
        assert!(declarations(&json!({"packages":[package("../outside")]}), "windows").is_err());
        assert!(
            declarations(
                &json!({"packages":[package("same"),package("same")]}),
                "windows"
            )
            .is_err()
        );
        assert!(
            declarations(
                &json!({"packages":[package("games/future/adapter.dll")]}),
                "linux"
            )
            .unwrap()
            .is_empty()
        );
    }
}

fn inventory(root: &Path) -> Result<Vec<Value>, String> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        for e in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            if e.file_type().map_err(|e| e.to_string())?.is_dir() {
                pending.push(e.path());
            } else {
                let bytes = fs::read(e.path()).map_err(|e| e.to_string())?;
                let path = e
                    .path()
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push(json!({"path":path,"size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}));
            }
        }
    }
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(files)
}
fn check_output_path(root: &Path, path: &Path) -> Result<(), String> {
    let intended = root
        .join("target/dist")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let resolved = if path.exists() {
        path.canonicalize().map_err(|e| e.to_string())?
    } else {
        path.parent()
            .ok_or("Missing output parent")?
            .canonicalize()
            .map_err(|e| e.to_string())?
            .join(path.file_name().ok_or("Missing output name")?)
    };
    if !resolved.starts_with(&intended) || resolved == intended {
        return Err("Refusing a directory move outside distribution output".into());
    }
    Ok(())
}
