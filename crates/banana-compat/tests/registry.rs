//! Registry invariants: every game is reachable and self-consistent.
use std::path::Path;

#[test]
fn missing_games_are_reported_individually() {
    banana_compat::install();
    let reports = banana_core::scan(Path::new("/nonexistent-arcade-test-root"));
    assert_eq!(reports.len(), banana_compat::games().len());
    assert!(reports.iter().all(|r| r.binary.is_none() && r.error.is_some()));
}

#[test]
fn games_have_unique_ids_and_matching_profiles() {
    banana_compat::install();
    let games = banana_api::games();
    for (i, game) in games.iter().enumerate() {
        assert!(games[..i].iter().all(|g| g.id() != game.id()), "duplicate {}", game.id());
        if let Some(profile) = game.settings_profile() {
            assert_eq!(profile.id, game.config_key());
        }
        if let Some(layout) = game.control_layout() {
            assert_eq!(layout.id, game.config_key());
        }
        assert!(!game.backends().is_empty(), "{} declares no backend", game.id());
        for command in game.cli_commands() {
            assert!(command.flag.starts_with("--"));
        }
    }
    // Every CLI flag is unique across games.
    let mut flags: Vec<_> = games.iter().flat_map(|g| g.cli_commands()).map(|c| c.flag).collect();
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
