//! Exercise the public CLI with real archives and synthetic, inspectable ELFs.
use serde_json::Value;
use std::{fs, path::Path, process::Command};

#[test]
fn relocated_distribution_scans_archives_without_using_checkout_paths() {
    let temp =
        tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
    let bundle = temp.path().join("Moved Banana Pepper [portable]");
    fs::create_dir(&bundle).unwrap();
    fs::write(bundle.join("banana-pepper.dist"), "v1").unwrap();
    let executable = bundle.join(format!("arcade-desktop{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_arcade-desktop"), &executable).unwrap();
    let info = Command::new(&executable)
        .env("LOCALAPPDATA", temp.path().join("user/local"))
        .env("APPDATA", temp.path().join("user/config"))
        .env("XDG_CONFIG_HOME", temp.path().join("user/config"))
        .env("XDG_DATA_HOME", temp.path().join("user/data"))
        .env("XDG_STATE_HOME", temp.path().join("user/state"))
        .env("XDG_CACHE_HOME", temp.path().join("user/cache"))
        .arg("--runtime-info")
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        info.status.success(),
        "{}",
        String::from_utf8_lossy(&info.stderr)
    );
    let info: Value = serde_json::from_slice(&info.stdout).unwrap();
    assert_eq!(info["distribution"], true);
    assert_eq!(
        Path::new(info["resources"].as_str().unwrap()),
        bundle.canonicalize().unwrap()
    );
    let source = temp.path().join("game");
    fs::create_dir(&source).unwrap();
    fs::create_dir(source.join("data")).unwrap();
    fs::write(source.join("game"), elf(64)).unwrap();
    let archive = temp.path().join("game.zar");
    zarchive_rs::pack_dir(&source, &archive).unwrap();
    let scan = Command::new(&executable)
        .env("LOCALAPPDATA", temp.path().join("user/local"))
        .env("APPDATA", temp.path().join("user/config"))
        .env("XDG_CONFIG_HOME", temp.path().join("user/config"))
        .arg("--scan")
        .arg(&archive)
        .env_remove("ARCADE_ARCHIVE_CACHE")
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        scan.status.success(),
        "{}",
        String::from_utf8_lossy(&scan.stderr)
    );
    let reports: Value = serde_json::from_slice(&scan.stdout).unwrap();
    assert_eq!(profile(&reports, "minecraft")["binary"]["bits"], 64);
    assert!(!bundle.join("runtime").exists());
    assert!(!temp.path().join("runtime").exists());
    assert!(!temp.path().join("archive-cache").exists());
}

fn elf(bits: u8) -> Vec<u8> {
    let mut b = vec![0; if bits == 64 { 64 } else { 52 }];
    b[..7].copy_from_slice(if bits == 64 {
        b"\x7fELF\x02\x01\x01"
    } else {
        b"\x7fELF\x01\x01\x01"
    });
    b[16..18].copy_from_slice(&2u16.to_le_bytes());
    b[18..20].copy_from_slice(&(if bits == 64 { 62u16 } else { 3u16 }).to_le_bytes());
    b[20..24].copy_from_slice(&1u32.to_le_bytes());
    if bits == 64 {
        b[52..54].copy_from_slice(&64u16.to_le_bytes());
        b[54..56].copy_from_slice(&56u16.to_le_bytes());
        b[58..60].copy_from_slice(&64u16.to_le_bytes());
    } else {
        b[40..42].copy_from_slice(&52u16.to_le_bytes());
        b[42..44].copy_from_slice(&32u16.to_le_bytes());
        b[46..48].copy_from_slice(&40u16.to_le_bytes());
    }
    b
}
fn scan(source: &Path, cache: &Path) -> std::process::Output {
    let user = cache.with_extension("user");
    Command::new(env!("CARGO_BIN_EXE_arcade-desktop"))
        .env("LOCALAPPDATA", user.join("local"))
        .env("APPDATA", user.join("config"))
        .env("XDG_CONFIG_HOME", user.join("config"))
        .env("XDG_DATA_HOME", user.join("data"))
        .env("XDG_STATE_HOME", user.join("state"))
        .env("XDG_CACHE_HOME", user.join("cache"))
        .arg("--scan")
        .arg(source)
        .output()
        .unwrap()
}
fn profile<'a>(reports: &'a Value, id: &str) -> &'a Value {
    reports
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["profile"]["id"] == id)
        .unwrap()
}

#[test]
fn archived_library_inspects_the_same_games_as_its_original_folder() {
    let temp =
        tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
    let library = temp.path().join("library");
    for (name, bits) in [
        ("linux based/Big Bug Hunter Pro/game", 32),
        ("linux based/Aliens Armageddon/game", 32),
        ("linux based/Minecraft Dungeons Arcade/gamedump.exec", 64),
    ] {
        let path = library.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, elf(bits)).unwrap();
    }
    fs::create_dir_all(library.join("linux based/Minecraft Dungeons Arcade/data")).unwrap();
    let archive = temp.path().join("normal library.zar");
    zarchive_rs::pack_dir(&library, &archive).unwrap();
    let cache = temp.path().join("cache");
    let plain = scan(&library, &cache);
    assert!(plain.status.success());
    assert!(!cache.exists());
    let packed = scan(&archive, &cache);
    assert!(
        packed.status.success(),
        "{}",
        String::from_utf8_lossy(&packed.stderr)
    );
    let plain: Value = serde_json::from_slice(&plain.stdout).unwrap();
    let packed: Value = serde_json::from_slice(&packed.stdout).unwrap();
    for id in ["big-buck", "aliens", "minecraft"] {
        assert_eq!(
            profile(&plain, id)["binary"],
            profile(&packed, id)["binary"]
        );
        assert!(profile(&packed, id)["binary"].is_object());
        assert!(Path::new(profile(&packed, id)["path"].as_str().unwrap()).starts_with(&archive));
        assert!(!cache.exists());
    }
}

#[test]
fn direct_minecraft_archive_does_not_depend_on_the_cache_or_archive_name() {
    let temp =
        tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(source.join("data")).unwrap();
    // An original ELF64 named "game", no .hvc marker and no title in the path.
    fs::write(source.join("game"), elf(64)).unwrap();
    let archive = temp.path().join("arbitrary.zar");
    zarchive_rs::pack_dir(&source, &archive).unwrap();
    let output = scan(&archive, &temp.path().join("cache"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(profile(&reports, "minecraft")["binary"]["bits"], 64);
}

#[test]
fn invalid_archive_is_a_cli_failure_with_a_useful_error() {
    let temp =
        tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
    let archive = temp.path().join("broken.zar");
    fs::write(&archive, b"not a ZArchive").unwrap();
    let output = scan(&archive, &temp.path().join("cache"));
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("Cannot load ZArchive"), "{message}");
    assert!(message.contains("broken.zar"));
}
