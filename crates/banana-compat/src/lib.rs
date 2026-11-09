//! The built-in game compatibility crates and execution backends.
//!
//! This is the only shared component that names individual games. Adding a
//! game means creating its crate under `game-compat/` and listing it here.
use banana_api::{ExecutionBackend, GameCompat};
use std::sync::Once;

/// Every built-in game, in library order.
pub fn games() -> Vec<&'static dyn GameCompat> {
    let mut games: Vec<&'static dyn GameCompat> = vec![
        &banana_game_aliens_armageddon::GAME,
        &banana_game_jurassic_park::GAME,
        &banana_game_big_buck_hunter::GAME,
        &banana_game_big_buck_world::GAME,
        &banana_game_super_cars::GAME,
    ];
    // Minecraft remains desktop-only until Android work is explicitly requested.
    #[cfg(not(target_os = "android"))]
    games.push(&banana_game_minecraft_dungeons::GAME);
    games.push(&banana_game_ford_racing::GAME);
    // Skylanders runs under Wine on the desktop; no Android launch plan yet.
    #[cfg(not(target_os = "android"))]
    games.push(&banana_game_skylanders_cloud_patrol::GAME);
    // A 64-bit Windows build: desktop Wine only.
    #[cfg(not(target_os = "android"))]
    games.push(&banana_game_wangan_midnight_6r::GAME);
    games
}

/// Every built-in execution backend.
pub fn backends() -> Vec<&'static dyn ExecutionBackend> {
    #[cfg(target_os = "android")]
    return vec![&banana_android::NATIVE, &banana_android::WINE];
    #[cfg(not(target_os = "android"))]
    vec![
        &banana_native::BACKEND,
        &banana_wine::BACKEND,
        &banana_water::BACKEND,
    ]
}

/// Register all built-in games with the API, settings and input systems.
/// Frontends call this once at startup; repeated calls are harmless.
pub fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let games = games();
        banana_settings::install(games.iter().filter_map(|g| g.settings_profile()).collect());
        banana_input::controls::install(games.iter().filter_map(|g| g.control_layout()).collect());
        banana_api::install(games, backends());
    });
}
