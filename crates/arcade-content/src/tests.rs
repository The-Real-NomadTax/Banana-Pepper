use super::*;
use std::{io::Write, sync::Arc};
use zarchive_rs::ArchiveWriter;

fn write_archive(path: &Path, files: &[(&str, &[u8])]) {
    let mut writer = ArchiveWriter::new(File::create(path).unwrap()).unwrap();
    for (name, bytes) in files {
        if let Some((parent, _)) = name.rsplit_once('/') {
            writer.create_dir_all(parent).unwrap();
        }
        writer.start_file(name).unwrap().write_all(bytes).unwrap();
    }
    writer.create_dir_all("empty/nested").unwrap();
    writer.finish().unwrap();
}
use std::fs::File;

#[test]
fn normal_folders_and_missing_library_paths_keep_existing_behaviour() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("unused-cache");
    assert_eq!(prepare(temp.path(), &cache).unwrap(), temp.path());
    let missing = temp.path().join("games");
    assert_eq!(prepare(&missing, &cache).unwrap(), missing);
    assert!(!cache.exists());
    assert!(prepare(&temp.path().join("missing.ZAR"), &cache).is_err());
}

#[test]
fn extracts_multiblock_data_unicode_and_empty_directories_without_altering_archive() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("game.zar");
    let data: Vec<u8> = (0..200_000)
        .map(|i| ((i * 37 + i / 17) % 256) as u8)
        .collect();
    write_archive(
        &archive,
        &[
            ("FordRacing/fordracing.exe", b"MZ fixture"),
            ("data/caf\u{e9}/large.bin", &data),
        ],
    );
    let original = fs::read(&archive).unwrap();
    let cache = temp.path().join("cache");
    let root = prepare(&archive, &cache).unwrap();
    assert_eq!(
        fs::read(root.join("data/caf\u{e9}/large.bin")).unwrap(),
        data
    );
    assert!(root.join("empty/nested").is_dir());
    assert_eq!(
        fs::read(root.join("FordRacing/fordracing.exe")).unwrap(),
        b"MZ fixture"
    );
    assert_eq!(fs::read(&archive).unwrap(), original);
    let modified = fs::metadata(root.join("data/caf\u{e9}/large.bin"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(prepare(&archive, &cache).unwrap(), root);
    assert_eq!(
        fs::metadata(root.join("data/caf\u{e9}/large.bin"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
    // Content addressing reuses the verified extraction even after a rename.
    let renamed = temp.path().join("renamed.zarchive");
    fs::rename(&archive, &renamed).unwrap();
    assert_eq!(prepare(&renamed, &cache).unwrap(), root);
}

#[test]
fn replacing_an_archive_uses_new_content_and_keeps_the_old_cache_intact() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("game.zar");
    let cache = temp.path().join("cache");
    write_archive(&archive, &[("game", b"version one")]);
    let first = prepare(&archive, &cache).unwrap();
    write_archive(&archive, &[("game", b"version two")]);
    let second = prepare(&archive, &cache).unwrap();
    assert_ne!(first, second);
    assert_eq!(fs::read(first.join("game")).unwrap(), b"version one");
    assert_eq!(fs::read(second.join("game")).unwrap(), b"version two");
}

#[test]
fn corrupt_payload_is_rejected_before_any_extraction_is_published() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("corrupt.zar");
    let cache = temp.path().join("cache");
    write_archive(&archive, &[("game", b"payload to corrupt")]);
    let mut bytes = fs::read(&archive).unwrap();
    bytes[0] ^= 1;
    fs::write(&archive, bytes).unwrap();
    let error = prepare(&archive, &cache).unwrap_err();
    assert!(error.to_lowercase().contains("integrity"), "{error}");
    assert!(
        fs::read_dir(&cache)
            .unwrap()
            .all(|e| !e.unwrap().file_type().unwrap().is_dir())
    );
}

#[test]
fn simultaneous_loads_publish_one_complete_tree() {
    let temp = Arc::new(tempfile::tempdir().unwrap());
    write_archive(
        &temp.path().join("game.zar"),
        &[("bbh3data/scene", &[3; 70_000]), ("game", b"fixture")],
    );
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let temp = Arc::clone(&temp);
            std::thread::spawn(move || {
                prepare(&temp.path().join("game.zar"), &temp.path().join("cache")).unwrap()
            })
        })
        .collect();
    let roots: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(roots.iter().all(|r| r == &roots[0]));
    assert_eq!(
        fs::read(roots[0].join("bbh3data/scene")).unwrap(),
        [3; 70_000]
    );
    assert_eq!(
        fs::read_dir(temp.path().join("cache"))
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_type().unwrap().is_dir())
            .count(),
        1
    );
}

#[test]
fn incomplete_or_redirected_cache_is_not_treated_as_ready() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("game.zar");
    let cache = temp.path().join("cache");
    write_archive(&archive, &[("game", b"fixture")]);
    let root = prepare(&archive, &cache).unwrap();
    fs::remove_file(root.parent().unwrap().join("complete")).unwrap();
    assert!(
        prepare(&archive, &cache)
            .unwrap_err()
            .contains("Incomplete archive cache")
    );
    assert_eq!(fs::read(root.join("game")).unwrap(), b"fixture");
}

#[test]
fn deep_or_nonportable_paths_fail_before_creating_game_files() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let archive = temp.path().join("unsafe.zar");
    write_archive(&archive, &[("assets/save:stream", b"unexpected stream")]);
    assert!(
        prepare(&archive, &cache)
            .unwrap_err()
            .contains("Unsafe archive path")
    );
    let deep = format!("{}game", "a/".repeat(66));
    write_archive(&archive, &[(&deep, b"too deep")]);
    assert!(
        prepare(&archive, &cache)
            .unwrap_err()
            .contains("nesting exceeds")
    );
    assert!(
        fs::read_dir(&cache)
            .unwrap()
            .all(|e| !e.unwrap().file_type().unwrap().is_dir())
    );
}

#[cfg(windows)]
#[test]
fn windows_device_aliases_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("device.zar");
    write_archive(
        &archive,
        &[("assets/NUL.txt", b"not a regular Windows filename")],
    );
    assert!(
        prepare(&archive, &temp.path().join("cache"))
            .unwrap_err()
            .contains("cannot be represented on Windows")
    );
}

#[cfg(unix)]
#[test]
fn linux_executables_and_interpreter_scripts_regain_execute_permission() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("linux.zar");
    write_archive(
        &archive,
        &[
            ("game", b"\x7fELFfixture"),
            ("helper", b"#!/bin/sh\nexit 0\n"),
            ("assets/data", b"data"),
        ],
    );
    let root = prepare(&archive, &temp.path().join("cache")).unwrap();
    for name in ["game", "helper"] {
        assert_eq!(
            fs::metadata(root.join(name)).unwrap().permissions().mode() & 0o111,
            0o111
        );
    }
    assert_eq!(
        fs::metadata(root.join("assets/data"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
}

#[test]
fn converter_round_trips_through_the_game_loader() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(source.join("data/café/empty")).unwrap();
    fs::write(source.join("game.exe"), b"MZ game").unwrap();
    let data: Vec<u8> = (0..200_000).map(|i| (i * 37 % 251) as u8).collect();
    fs::write(source.join("data/café/assets.bin"), &data).unwrap();
    fs::write(source.join("zero"), []).unwrap();
    let output = temp.path().join("game.zar");
    let mut last = pack::Progress::default();
    pack::convert(&source, &output, &false.into(), |p| last = p).unwrap();
    assert_eq!(last.phase, "Complete");
    assert_eq!(last.files_done, 3);
    assert_eq!(last.bytes_done, last.bytes_total);
    let tree = prepare(&output, &temp.path().join("cache")).unwrap();
    assert_eq!(fs::read(tree.join("game.exe")).unwrap(), b"MZ game");
    assert_eq!(fs::read(tree.join("data/café/assets.bin")).unwrap(), data);
    assert!(tree.join("data/café/empty").is_dir());
    assert_eq!(fs::metadata(tree.join("zero")).unwrap().len(), 0);
    assert_eq!(fs::read(source.join("data/café/assets.bin")).unwrap(), data);
}

#[test]
fn converter_rejects_overwrite_and_output_inside_source() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    let output = temp.path().join("game.zar");
    fs::write(&output, b"keep existing").unwrap();
    assert!(
        pack::convert(&source, &output, &false.into(), |_| {})
            .unwrap_err()
            .contains("already exists")
    );
    assert_eq!(fs::read(&output).unwrap(), b"keep existing");
    assert!(
        pack::convert(&source, &source.join("game.zar"), &false.into(), |_| {})
            .unwrap_err()
            .contains("outside")
    );
    assert_eq!(fs::read_dir(&source).unwrap().count(), 0);
}

#[test]
fn converter_cancellation_cleans_partial_output() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("game"), vec![42; 2 * 1024 * 1024]).unwrap();
    let output = temp.path().join("game.zar");
    let cancel = AtomicBool::new(false);
    let result = pack::convert(&source, &output, &cancel, |p| {
        if p.bytes_done > 0 {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert!(result.unwrap_err().contains("cancelled"));
    assert!(!output.exists());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn converter_rejects_case_collisions_and_cleans_up_on_changed_input() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    let file = source.join("game");
    fs::write(&file, b"original").unwrap();
    let output = temp.path().join("game.zar");
    let result = pack::convert(&source, &output, &false.into(), |p| {
        if p.phase == "Compressing" && p.bytes_done == 0 {
            fs::write(&file, b"changed size").unwrap();
        }
    });
    assert!(result.unwrap_err().contains("changed"));
    assert!(!output.exists());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    // Only case-sensitive filesystems can represent both source names.
    fs::write(source.join("GAME"), b"collision").unwrap();
    if fs::read_dir(&source).unwrap().count() == 2 {
        assert!(
            pack::convert(&source, &output, &false.into(), |_| {})
                .unwrap_err()
                .contains("case")
        );
    }
}

#[cfg(unix)]
#[test]
fn converter_rejects_links_without_following_them() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    std::os::unix::fs::symlink(temp.path(), source.join("loop")).unwrap();
    let output = temp.path().join("game.zar");
    assert!(
        pack::convert(&source, &output, &false.into(), |_| {})
            .unwrap_err()
            .contains("Links")
    );
    assert!(!output.exists());
}

#[test]
fn converter_can_pack_an_empty_folder_and_refuses_a_late_output_collision() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    let output = temp.path().join("game.zar");
    pack::convert(&source, &output, &false.into(), |_| {}).unwrap();
    let tree = prepare(&output, &temp.path().join("cache")).unwrap();
    assert_eq!(fs::read_dir(tree).unwrap().count(), 0);
    fs::remove_file(&output).unwrap();
    let result = pack::convert(&source, &output, &false.into(), |p| {
        if p.phase == "Verifying archive" {
            fs::write(&output, b"another writer").unwrap();
        }
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&output).unwrap(), b"another writer");
}
