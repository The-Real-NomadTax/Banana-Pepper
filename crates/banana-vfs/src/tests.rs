use super::*;
use banana_content::{ContentSource, MemorySource};
use std::{collections::BTreeMap, fs};
fn source() -> Arc<dyn ContentSource> {
    Arc::new(MemorySource {
        files: BTreeMap::from([
            ("large".into(), Arc::from(vec![b'x'; 1_000_000])),
            ("dir/seed".into(), Arc::from(&b"seed"[..])),
        ]),
    })
}
fn view(t: &Path) -> View {
    View::new(source(), t, "build1", BTreeMap::new(), BTreeMap::new()).unwrap()
}
#[test]
fn untouched_content_is_never_materialized_and_changes_persist() {
    let t =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    {
        let v = view(t.path());
        let mut b = [0; 4];
        v.read("large", 999_990, &mut b).unwrap();
        assert_eq!(&b, b"xxxx");
        assert_eq!(fs::read_dir(t.path().join("blocks")).unwrap().count(), 0);
        v.write("large", 999_990, b"done").unwrap();
        assert_eq!(fs::read_dir(t.path().join("blocks")).unwrap().count(), 1);
    }
    let v = view(t.path());
    let mut b = [0; 4];
    v.read("large", 999_990, &mut b).unwrap();
    assert_eq!(&b, b"done");
    assert!(!t.path().join("large").exists());
}
#[test]
fn rename_delete_and_sparse_truncation_survive_restart() {
    let t =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    {
        let v = view(t.path());
        v.rename("dir", "new", false).unwrap();
        assert!(v.metadata("dir/seed").is_err());
        assert_eq!(v.read_dir("new").unwrap()[0].name, "seed");
        let mut b = [0; 4];
        v.read("new/seed", 0, &mut b).unwrap();
        assert_eq!(&b, b"seed");
        v.truncate("new/seed", 2).unwrap();
        v.truncate("new/seed", 8).unwrap();
        let mut b = [9; 8];
        v.read("new/seed", 0, &mut b).unwrap();
        assert_eq!(&b, b"se\0\0\0\0\0\0");
        v.remove("new/seed").unwrap();
        v.remove("new").unwrap();
    }
    let v = view(t.path());
    assert!(v.metadata("new").is_err());
    assert!(v.metadata("dir").is_err());
}
#[test]
fn source_identity_and_single_writer_are_enforced() {
    let t =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let v = view(t.path());
    assert!(
        View::new(
            source(),
            t.path(),
            "build1",
            BTreeMap::new(),
            BTreeMap::new()
        )
        .is_err()
    );
    v.create("save", false).unwrap();
    v.write("save", 0, b"ok").unwrap();
    drop(v);
    assert!(
        View::new(
            source(),
            t.path(),
            "build2",
            BTreeMap::new(),
            BTreeMap::new()
        )
        .is_err()
    );
}
#[test]
fn transforms_and_aliases_share_the_overlay() {
    let t =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let v = View::new(
        source(),
        t.path(),
        "build1",
        BTreeMap::from([("patched".into(), Arc::from(&b"MZpatch"[..]))]),
        BTreeMap::from([("alias".into(), "dir".into())]),
    )
    .unwrap();
    assert!(v.metadata("patched").unwrap().executable);
    v.write("alias/seed", 0, b"new!").unwrap();
    let mut b = [0; 4];
    v.read("dir/seed", 0, &mut b).unwrap();
    assert_eq!(&b, b"new!");
    assert!(v.create("../escape", false).is_err());
}

#[test]
fn open_handles_survive_unlink_and_share_unlinked_writes() {
    let t =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let v = Arc::new(view(t.path()));
    let a = v.open_file("dir/seed").unwrap();
    let b = v.open_file("dir/seed").unwrap();
    let identity = a.identity();
    v.rename("dir/seed", "renamed", false).unwrap();
    assert_eq!(v.identity("renamed").unwrap(), identity);
    v.remove("renamed").unwrap();
    a.write(0, b"kept").unwrap();
    let mut bytes = [0; 4];
    b.read(0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"kept");
    v.create("renamed", false).unwrap();
    assert_ne!(v.identity("renamed").unwrap(), identity);
    assert_eq!(a.metadata().unwrap().len, 4);
}
#[test]
fn metadata_and_case_rules_are_shared_by_all_providers() {
    let t =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let s: Arc<dyn ContentSource> = Arc::new(MemorySource {
        files: BTreeMap::from([("Data/Save.INI".into(), Arc::from(&b"original"[..]))]),
    });
    {
        let v = View::with_case(
            s.clone(),
            t.path(),
            "case-build",
            BTreeMap::new(),
            BTreeMap::new(),
            false,
        )
        .unwrap();
        assert_eq!(v.metadata("data/save.ini").unwrap().len, 8);
        v.set_attributes(
            "DATA/SAVE.INI",
            Attributes {
                mode: Some(0o600),
                modified_ns: 10000000,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(fs::read_dir(t.path().join("blocks")).unwrap().count(), 0);
    }
    let v = View::with_case(
        s,
        t.path(),
        "case-build",
        BTreeMap::new(),
        BTreeMap::new(),
        false,
    )
    .unwrap();
    assert_eq!(v.attributes("Data/Save.INI").unwrap().mode, Some(0o600));
    assert_eq!(v.attributes("data/save.ini").unwrap().modified_ns, 10000000);
}

#[test]
fn stale_mount_cleanup_respects_live_locks_and_nonempty_directories() {
    let t =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    for name in ["dead", "live", "occupied"] {
        fs::create_dir(t.path().join(name)).unwrap();
        fs::write(
            t.path().join(format!("{name}.banana-mount-lock")),
            "banana-mount-v1\n",
        )
        .unwrap();
    }
    fs::write(t.path().join("occupied/keep"), b"never delete contents").unwrap();
    let live = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(t.path().join("live.banana-mount-lock"))
        .unwrap();
    live.try_lock().unwrap();
    cleanup_stale_mounts(t.path()).unwrap();
    assert!(!t.path().join("dead").exists());
    assert!(t.path().join("live").exists());
    assert!(t.path().join("occupied/keep").exists());
    drop(live);
    cleanup_stale_mounts(t.path()).unwrap();
    assert!(!t.path().join("live").exists());
}

#[test]
fn reset_preserves_the_source_and_cannot_race_an_active_session() {
    let t =
        tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    {
        let v = view(t.path());
        v.write("large", 0, b"changed").unwrap();
        assert!(reset_overlay(t.path(), Some("large")).is_err());
    }
    reset_overlay(t.path(), Some("large")).unwrap();
    {
        let v = view(t.path());
        let mut b = [0; 7];
        v.read("large", 0, &mut b).unwrap();
        assert_eq!(&b, b"xxxxxxx");
        v.create("save", false).unwrap();
        v.write("save", 0, b"saved").unwrap();
    }
    reset_overlay(t.path(), None).unwrap();
    assert!(!t.path().join("index.json").exists());
    assert_eq!(fs::read_dir(t.path().join("blocks")).unwrap().count(), 0);
}
