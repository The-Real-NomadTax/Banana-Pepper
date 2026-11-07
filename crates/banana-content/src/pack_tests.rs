use super::*;
use std::{fs, path::Path};
#[test]
fn converter_round_trips_through_the_game_loader() {
    let temp =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
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
    let tree = open(&output).unwrap();
    assert_eq!(
        read_all(tree.as_ref(), "game.exe", 256 * 1024 * 1024).unwrap(),
        b"MZ game"
    );
    assert_eq!(
        read_all(tree.as_ref(), "data/café/assets.bin", 256 * 1024 * 1024).unwrap(),
        data
    );
    assert!(tree.metadata("data/café/empty").unwrap().directory);
    assert_eq!(tree.metadata("zero").unwrap().len, 0);
    assert_eq!(fs::read(source.join("data/café/assets.bin")).unwrap(), data);
}

#[test]
fn converter_rejects_overwrite_and_output_inside_source() {
    let temp =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
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
    let temp =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
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
    let temp =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
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
fn converter_rejects_links_to_ancestor_directories() {
    let temp =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    std::os::unix::fs::symlink(&source, source.join("loop")).unwrap();
    let output = temp.path().join("game.zar");
    assert!(
        pack::convert(&source, &output, &false.into(), |_| {})
            .unwrap_err()
            .contains("cycle")
    );
    assert!(!output.exists());
}

#[test]
fn converter_can_pack_an_empty_folder_and_refuses_a_late_output_collision() {
    let temp =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    let output = temp.path().join("game.zar");
    pack::convert(&source, &output, &false.into(), |_| {}).unwrap();
    let tree = open(&output).unwrap();
    assert_eq!(tree.read_dir("").unwrap().len(), 0);
    drop(tree);
    fs::remove_file(&output).unwrap();
    let result = pack::convert(&source, &output, &false.into(), |p| {
        if p.phase == "Verifying archive" {
            fs::write(&output, b"another writer").unwrap();
        }
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&output).unwrap(), b"another writer");
}

#[cfg(unix)]
#[test]
fn converter_materializes_links_and_reports_broken_links() {
    use std::os::unix::fs::symlink;
    let temp =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let source = temp.path().join("source");
    let external = temp.path().join("external");
    fs::create_dir(&source).unwrap();
    fs::create_dir_all(external.join("empty")).unwrap();
    fs::write(external.join("game"), b"linked bytes").unwrap();
    symlink(&external, source.join("ElfLdr2")).unwrap();
    symlink(external.join("game"), source.join("game-link")).unwrap();
    symlink("game-link", source.join("chained-link")).unwrap();
    symlink("../../../pmuser", source.join("pmuser")).unwrap();
    // Separate aliases to one directory are allowed; only ancestor cycles fail.
    symlink(&external, source.join("alias")).unwrap();
    let output = temp.path().join("game.zar");
    let mut progress = pack::Progress::default();
    pack::convert(&source, &output, &false.into(), |p| progress = p).unwrap();
    assert_eq!(progress.skipped_links, ["pmuser"]);
    assert!(progress.skipped_links_message().contains("pmuser"));
    assert_eq!(progress.files_done, 4);
    assert_eq!(progress.bytes_done, progress.bytes_total);
    let tree = open(&output).unwrap();
    for path in ["ElfLdr2/game", "alias/game", "game-link", "chained-link"] {
        assert_eq!(
            read_all(tree.as_ref(), path, 256 * 1024 * 1024).unwrap(),
            b"linked bytes"
        );
        assert!(!tree.metadata(path).unwrap().directory);
    }
    assert!(tree.metadata("ElfLdr2/empty").unwrap().directory);
    assert!(tree.metadata("pmuser").is_err());
    assert!(
        fs::symlink_metadata(source.join("pmuser"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(external.join("game")).unwrap(), b"linked bytes");
}

#[cfg(unix)]
#[test]
fn converter_rejects_retargeted_links_and_special_files() {
    use std::os::unix::fs::symlink;
    let temp =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    fs::write(&first, b"first").unwrap();
    fs::write(&second, b"other").unwrap();
    let link = source.join("link");
    symlink(&first, &link).unwrap();
    let output = temp.path().join("game.zar");
    let result = pack::convert(&source, &output, &false.into(), |p| {
        if p.phase == "Compressing" && p.bytes_done == 0 {
            fs::remove_file(&link).unwrap();
            symlink(&second, &link).unwrap();
        }
    });
    assert!(result.unwrap_err().contains("changed"));
    assert!(!output.exists());
    fs::remove_file(&link).unwrap();
    symlink("/dev/null", source.join("device")).unwrap();
    assert!(
        pack::convert(&source, &output, &false.into(), |_| {})
            .unwrap_err()
            .contains("Special")
    );
    assert!(!output.exists());
}
