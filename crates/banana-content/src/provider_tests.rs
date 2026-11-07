use super::*;
use std::{collections::BTreeMap, fs, io::Write, sync::Arc};
fn contract(source: &dyn ContentSource) {
    assert!(source.metadata("").unwrap().directory);
    assert_eq!(source.read_dir("").unwrap()[0].name, "assets");
    assert_eq!(
        read_all(source, "assets/test.bin", 100).unwrap(),
        b"0123456789"
    );
    let f = source.open("assets/test.bin").unwrap();
    let mut b = [0; 3];
    assert_eq!(f.read_at(4, &mut b).unwrap(), 3);
    assert_eq!(&b, b"456");
    assert_eq!(f.read_at(100, &mut b).unwrap(), 0);
    assert!(source.open("../outside").is_err());
    assert!(source.metadata("/assets").is_err());
}
#[test]
fn folder_archive_memory_have_the_same_contract() {
    let t = tempfile::tempdir().unwrap();
    let folder = t.path().join("folder");
    fs::create_dir_all(folder.join("assets")).unwrap();
    fs::write(folder.join("assets/test.bin"), b"0123456789").unwrap();
    let archive = t.path().join("game.zar");
    let mut writer = zarchive_rs::ArchiveWriter::new(fs::File::create(&archive).unwrap()).unwrap();
    writer.create_dir_all("assets").unwrap();
    writer
        .start_file("assets/test.bin")
        .unwrap()
        .write_all(b"0123456789")
        .unwrap();
    writer.finish().unwrap();
    let before = fs::read(&archive).unwrap();
    contract(&Folder::new(&folder).unwrap());
    contract(&Zar::new(&archive).unwrap());
    contract(&MemorySource {
        files: BTreeMap::from([("assets/test.bin".into(), Arc::from(&b"0123456789"[..]))]),
    });
    assert_eq!(fs::read(archive).unwrap(), before);
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 2);
}
#[test]
fn folder_link_cannot_escape() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("root");
    fs::create_dir(&root).unwrap();
    assert!(Folder::new(&root).unwrap().open("../outside").is_err());
}
#[test]
fn corrupt_archive_is_rejected_without_output() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("bad.zar");
    fs::write(&p, b"not an archive").unwrap();
    assert!(Zar::new(&p).is_err());
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 1);
}

#[test]
fn damaged_archive_data_fails_verification() {
    let t = tempfile::tempdir().unwrap();
    let game = t.path().join("game");
    fs::create_dir(&game).unwrap();
    fs::write(game.join("data.bin"), vec![7u8; 200_000]).unwrap();
    let p = t.path().join("game.zar");
    crate::pack::convert(&game, &p, &Default::default(), |_| {}).unwrap();
    Zar::new(&p).unwrap().verify().unwrap();
    let good = fs::read(&p).unwrap();
    // Damage anywhere is caught by opening (headers, table of contents) or
    // by the verification that accompanies every fresh fingerprint.
    for at in [16, good.len() / 2, good.len() - 1] {
        let mut bytes = good.clone();
        bytes[at] ^= 0xff;
        fs::write(&p, bytes).unwrap();
        assert!(Zar::new(&p).and_then(|z| z.verify()).is_err(), "byte {at}");
    }
}

#[test]
fn fingerprints_bind_content_and_a_new_provider_needs_no_game_changes() {
    struct TestProvider;
    impl ContentProvider for TestProvider {
        fn kind(&self) -> &'static str {
            "test"
        }
        fn accepts(&self, p: &std::path::Path) -> bool {
            p.extension().is_some_and(|e| e == "test")
        }
        fn open(&self, _: &std::path::Path) -> std::io::Result<Arc<dyn ContentSource>> {
            Ok(Arc::new(MemorySource {
                files: BTreeMap::from([("assets/test.bin".into(), Arc::from(&b"0123456789"[..]))]),
            }))
        }
    }
    let mut registry = ProviderRegistry::default();
    registry.register(Arc::new(TestProvider)).unwrap();
    let source = registry.open(std::path::Path::new("future.test")).unwrap();
    contract(source.as_ref());
    let first = source.fingerprint().unwrap();
    let different = MemorySource {
        files: BTreeMap::from([("assets/test.bin".into(), Arc::from(&b"0123456780"[..]))]),
    };
    assert_ne!(first, different.fingerprint().unwrap());
}
