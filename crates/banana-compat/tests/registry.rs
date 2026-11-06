//! Registry invariants: every game is reachable and self-consistent.
use std::path::Path;

#[test]
fn missing_games_are_reported_individually() {
    banana_compat::install();
    let reports = banana_core::scan(Path::new("/nonexistent-arcade-test-root"));
    assert_eq!(reports.len(), banana_compat::games().len());
    assert!(
        reports
            .iter()
            .all(|r| r.binary.is_none() && r.error.is_some())
    );
}

#[test]
fn games_have_unique_ids_and_matching_profiles() {
    banana_compat::install();
    let games = banana_api::games();
    for (i, game) in games.iter().enumerate() {
        assert!(
            games[..i].iter().all(|g| g.id() != game.id()),
            "duplicate {}",
            game.id()
        );
        if let Some(profile) = game.settings_profile() {
            assert_eq!(profile.id, game.config_key());
        }
        if let Some(layout) = game.control_layout() {
            assert_eq!(layout.id, game.config_key());
        }
        assert!(
            !game.backends().is_empty(),
            "{} declares no backend",
            game.id()
        );
        for command in game.cli_commands() {
            assert!(command.flag.starts_with("--"));
        }
    }
    // Every CLI flag is unique across games.
    let mut flags: Vec<_> = games
        .iter()
        .flat_map(|g| g.cli_commands())
        .map(|c| c.flag)
        .collect();
    flags.sort();
    let before = flags.len();
    flags.dedup();
    assert_eq!(before, flags.len());
}

#[test]
fn unknown_game_is_an_error() {
    banana_compat::install();
    assert!(banana_runtime::run("no-such-game", Path::new("."), "launch").is_err());
}

#[test]
fn registered_runtime_sources_do_not_invoke_or_embed_interpreters() {
    fn check(directory: &Path) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                check(&path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                for forbidden in [
                    "Command::new(\"python",
                    "Command::new(\"bash\"",
                    "Command::new(\"sh\"",
                    ".py\")",
                    ".sh\")",
                ] {
                    assert!(
                        !source.contains(forbidden),
                        "Interpreter runtime dependency in {}: {forbidden}",
                        path.display()
                    );
                }
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for directory in ["game-compat", "backends", "crates/banana-runtime/src"] {
        check(&root.join(directory));
    }
}
