use banana_api::Controls;

#[test]
fn android_profiles_declare_their_runtime_and_controls() {
    banana_compat::install();
    for game in banana_api::games() {
        let Some(profile) = game.android_profile() else {
            continue;
        };
        assert!(!profile.executable.is_empty(), "{} executable", game.id());
        assert!(
            profile.requirements.contains(&"usr/local/bin/box64"),
            "{} CPU runtime",
            game.id()
        );
        assert!(
            profile.requirements.contains(&"usr/local/bin/afbridge"),
            "{} display bridge",
            game.id()
        );
        assert!(
            game.control_layout().is_some(),
            "{} touch controls",
            game.id()
        );
        assert!(
            profile
                .requirements
                .iter()
                .all(|p| !p.starts_with('/') && !p.contains(".."))
        );
        if game.controls() == Controls::Driving {
            assert!(!profile.verified);
        }
    }
}

#[test]
fn android_preparers_reject_unknown_sources() {
    banana_compat::install();
    let root = std::env::temp_dir().join(format!("banana-android-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for game in banana_api::games()
        .iter()
        .filter(|g| g.android_profile().is_some())
    {
        let source = root.join("source").join(game.id());
        let work = root.join("work").join(game.id());
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        let executable = source.join(game.android_profile().unwrap().executable);
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"unrecognized image").unwrap();
        assert!(
            banana_content::open(&source)
                .map_err(|e| e.to_string())
                .and_then(|source| game.identify(source.as_ref()))
                .is_err(),
            "{} accepted unknown image",
            game.id()
        );
        assert_eq!(std::fs::read(executable).unwrap(), b"unrecognized image");
    }
    let verified_root = root.canonicalize().unwrap();
    assert!(verified_root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    std::fs::remove_dir_all(verified_root).unwrap();
}

#[test]
#[cfg(not(target_os = "android"))]
fn minecraft_is_excluded_from_android() {
    banana_compat::install();
    let game = banana_api::game("minecraft").unwrap();
    assert!(game.android_profile().is_none());
}
