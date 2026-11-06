//! Build output only: an allowlist prevents local games, saves, credentials,
//! caches and developer configuration from entering a distribution.
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn main() {
    if let Err(error) = package() {
        eprintln!("Packaging failed: {error}");
        std::process::exit(1);
    }
}

fn package() -> Result<(), String> {
    if std::env::args_os().len() != 1 {
        return Err("Usage: cargo run -p banana-dist --release --locked".into());
    }
    if !cfg!(any(windows, target_os = "linux")) || std::env::consts::ARCH != "x86_64" {
        return Err("Desktop distributions currently support Windows/Linux x86-64".into());
    }
    let project = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let dist = project.join("target/dist");
    fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    // Reject redirected output directories before any recursive cleanup.
    let resolved = dist.canonicalize().map_err(|e| e.to_string())?;
    if !resolved.starts_with(&project) {
        return Err("target/dist must stay inside this checkout".into());
    }
    let name = format!(
        "banana-pepper-{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let destination = resolved.join(&name);
    let staging = resolved.join(format!(".{name}-staging-{}", std::process::id()));
    fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = assemble(&project, &staging);
    if let Err(error) = result {
        fs::remove_dir_all(&staging).map_err(|e| format!("{error}; cleaning staging: {e}"))?;
        return Err(error);
    }
    // Only replace a distribution we generated, never an arbitrary directory.
    let previous = resolved.join(format!(".{name}-previous-{}", std::process::id()));
    if destination.exists() {
        if fs::symlink_metadata(&destination)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
            || !banana_runtime::is_distribution(&destination)
        {
            fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
            return Err(format!(
                "Refusing to replace an unmanaged directory: {}",
                destination.display()
            ));
        }
        fs::rename(&destination, &previous).map_err(|e| e.to_string())?;
    }
    if let Err(error) = fs::rename(&staging, &destination) {
        if previous.exists() {
            fs::rename(&previous, &destination)
                .map_err(|e| format!("Publishing: {error}; restoring previous package: {e}"))?;
        }
        fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
        return Err(format!("Publishing package: {error}"));
    }
    if previous.exists() {
        fs::remove_dir_all(&previous).map_err(|e| e.to_string())?;
    }
    println!("Distribution ready: {}", destination.display());
    Ok(())
}

fn copy(project: &Path, package: &Path, source: &str, destination: &str) -> Result<(), String> {
    let source = project.join(source);
    if !source.is_file() {
        return Err(format!(
            "Missing build artifact {}. Run just build first.",
            source.display()
        ));
    }
    let destination = package.join(destination);
    fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::copy(&source, &destination).map_err(|e| format!("Copying {}: {e}", source.display()))?;
    Ok(())
}

fn assemble(project: &Path, package: &Path) -> Result<(), String> {
    let executable = std::env::consts::EXE_SUFFIX;
    for binary in ["arcade-desktop", "arcade-zar"] {
        let name = format!("{binary}{executable}");
        copy(project, package, &format!("target/release/{name}"), &name)?;
    }
    copy(
        project,
        package,
        &format!("target/release/banana-minecraft-card{executable}"),
        &format!("helpers/banana-minecraft-card{executable}"),
    )?;
    for adapter in ["inpout32.dll", "arcade_wine_hook.dll"] {
        copy(
            project,
            package,
            &format!("target/i686-pc-windows-msvc/release/{adapter}"),
            &format!("adapters/windows-x86/{adapter}"),
        )?;
    }
    if cfg!(windows) {
        copy(
            project,
            package,
            "target/i686-pc-windows-msvc/release/water-buck.exe",
            "helpers/water-buck.exe",
        )?;
        copy(
            project,
            package,
            "target/release/water.exe",
            "helpers/water.exe",
        )?;
        if project.join("target/wsl/package-ready").is_file() {
            for name in ["banana-game-host", "banana-session"] {
                copy(
                    project,
                    package,
                    &format!("target/wsl/release/{name}"),
                    &format!("helpers/linux/{name}"),
                )?;
            }
            copy(
                project,
                package,
                "target/wsl/release/libarcade_minecraft_io.so",
                "adapters/linux-x86_64/libarcade_minecraft_io.so",
            )?;
            copy(
                project,
                package,
                "target/wsl/i686-unknown-linux-gnu/release/libarcade_linux_shim.so",
                "adapters/linux-x86/libarcade_linux_shim.so",
            )?;
        }
    } else {
        copy(
            project,
            package,
            "target/i686-unknown-linux-gnu/debug/libarcade_linux_shim.so",
            "adapters/linux-x86/libarcade_linux_shim.so",
        )?;
        copy(
            project,
            package,
            "target/debug/libarcade_minecraft_io.so",
            "adapters/linux-x86_64/libarcade_minecraft_io.so",
        )?;
        for name in ["banana-session", "banana-game-host"] {
            copy(
                project,
                package,
                &format!("target/release/{name}"),
                &format!("helpers/linux/{name}"),
            )?;
        }
    }
    for entry in fs::read_dir(project.join("docs")).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().extension().is_some_and(|e| e == "md") {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "Non-UTF-8 documentation name")?;
            copy(
                project,
                package,
                &format!("docs/{name}"),
                &format!("docs/{name}"),
            )?;
        }
    }
    banana_compat::install();
    fs::write(
        package.join("settings.cfg"),
        banana_settings::Settings::default().to_text(),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        package.join("controls.cfg"),
        banana_input::controls::Bindings::default().to_text(),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        package.join(banana_runtime::DIST_MARKER),
        "Banana Pepper portable distribution v1\n",
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        package.join("README.txt"),
        include_str!("../README-dist.txt"),
    )
    .map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    inventory(package, package, &mut files)?;
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    let manifest = serde_json::json!({
        "name": "Banana Pepper", "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "files": files,
        "external_dependencies": if cfg!(windows) {
            "Windows x86-64, graphics driver/OpenGL; optional WSL2/WSLg and Linux guest components for WSL backends"
        } else {
            "Linux x86-64, desktop graphics, Bubblewrap, 32-bit game libraries; Wine/DXVK for Windows titles; see README.txt"
        },
    });
    fs::write(
        package.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn inventory(
    root: &Path,
    directory: &Path,
    files: &mut Vec<serde_json::Value>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            inventory(root, &entry.path(), files)?;
        } else {
            let bytes = fs::read(entry.path()).map_err(|e| e.to_string())?;
            files.push(serde_json::json!({
                "path": entry.path().strip_prefix(root).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/"),
                "bytes": bytes.len(), "sha256": format!("{:x}", Sha256::digest(&bytes)),
            }));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_excludes_local_data_and_has_a_complete_verifiable_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("source");
        let package = temp.path().join("package");
        fs::create_dir_all(&package).unwrap();
        let executable = std::env::consts::EXE_SUFFIX;
        let mut sources = vec![
            format!("target/release/arcade-desktop{executable}"),
            format!("target/release/arcade-zar{executable}"),
            format!("target/release/banana-minecraft-card{executable}"),
            "target/i686-pc-windows-msvc/release/inpout32.dll".into(),
            "target/i686-pc-windows-msvc/release/arcade_wine_hook.dll".into(),
        ];
        if cfg!(windows) {
            sources.extend([
                "target/i686-pc-windows-msvc/release/water-buck.exe".into(),
                "target/release/water.exe".into(),
            ]);
        } else {
            sources.extend([
                "target/i686-unknown-linux-gnu/debug/libarcade_linux_shim.so".into(),
                "target/debug/libarcade_minecraft_io.so".into(),
                "target/release/banana-session".into(),
                "target/release/banana-game-host".into(),
            ]);
        }
        sources.extend([
            "docs/setup.md".into(),
            "settings.cfg".into(),
            "controls.cfg".into(),
            "games/private-game.exe".into(),
            "runtime/private-save.bin".into(),
            ".env".into(),
            "scripts/old-runtime.py".into(),
        ]);
        for source in sources {
            let path = project.join(source);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "private fixture data").unwrap();
        }
        assemble(&project, &package).unwrap();
        assert!(banana_runtime::is_distribution(&package));
        for absent in ["games", "runtime", ".env", "target", "scripts"] {
            assert!(
                !package.join(absent).exists(),
                "Unexpected local data: {absent}"
            );
        }
        assert!(
            banana_settings::Settings::parse(
                &fs::read_to_string(package.join("settings.cfg")).unwrap()
            )
            .is_ok()
        );
        assert!(
            banana_input::controls::Bindings::parse(
                &fs::read_to_string(package.join("controls.cfg")).unwrap()
            )
            .is_ok()
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(package.join("manifest.json")).unwrap()).unwrap();
        let files = manifest["files"].as_array().unwrap();
        for file in files {
            let path = Path::new(file["path"].as_str().unwrap());
            assert!(
                !path
                    .extension()
                    .is_some_and(|x| ["py", "sh", "ps1"].iter().any(|e| x == *e)),
                "Distributions must contain compiled runtime components"
            );
            let bytes = fs::read(package.join(file["path"].as_str().unwrap())).unwrap();
            assert_eq!(file["sha256"], format!("{:x}", Sha256::digest(&bytes)));
            assert_eq!(file["bytes"], bytes.len());
        }
        let mut actual = Vec::new();
        inventory(&package, &package, &mut actual).unwrap();
        assert_eq!(actual.len(), files.len() + 1); // manifest itself
        // Missing required artifacts are errors, never partial success.
        fs::remove_file(project.join(format!("target/release/arcade-desktop{executable}")))
            .unwrap();
        assert!(
            assemble(&project, &package)
                .unwrap_err()
                .contains("Missing build artifact")
        );
    }
}
