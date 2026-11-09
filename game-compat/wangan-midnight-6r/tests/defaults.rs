//! Maximum Tune 6R's default bindings, as the hook's JVS board reads them.
use banana_api::GameCompat;
use banana_game_wangan_midnight_6r::GAME;
use banana_input::controls::Bindings;

fn bindings() -> Bindings {
    banana_input::controls::install(vec![GAME.control_layout().unwrap()]);
    banana_settings::install(GAME.settings_profile().into_iter().collect());
    Bindings::default()
}

#[test]
fn defaults_are_valid_and_reach_the_board_targets() {
    let b = bindings();
    b.validate().unwrap();
    let keys = b.env_value("wmmt6r").unwrap();
    // Arrow keys steer and drive (DIK codes), E/Q shift, 5 is the coin.
    for pair in ["1:203", "2:205", "3:200", "4:208", "512:18", "513:16", "259:6"] {
        assert!(keys.split(',').any(|p| p == pair), "{pair} missing from {keys}");
    }
    let pad = b.pad_env_value("wmmt6r").unwrap();
    // Triggers are the pedals, bumpers shift.
    for pair in ["3:131072", "4:65536", "512:512", "513:256"] {
        assert!(pad.split(',').any(|p| p == pair), "{pair} missing from {pad}");
    }
    assert_eq!(b.pad_env_value_p2("wmmt6r").unwrap(), "");
}
