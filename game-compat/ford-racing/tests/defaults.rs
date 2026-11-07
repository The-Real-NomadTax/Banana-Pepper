//! Ford Racing's default settings and bindings. The adapter's built-in fallbacks
//! (used when no environment value is passed) must match these.
use banana_api::GameCompat;
use banana_game_ford_racing::GAME;
use banana_input::controls::Bindings;

fn bindings() -> Bindings {
    banana_input::controls::install(vec![GAME.control_layout().unwrap()]);
    banana_settings::install(GAME.settings_profile().into_iter().collect());
    Bindings::default()
}

#[test]
fn defaults_match_the_adapter_fallbacks() {
    let b = bindings();
    b.validate().unwrap();
    // hook.rs DEFAULT_PAD.
    assert_eq!(
        b.pad_env_value("ford").unwrap(),
        "1:4,2:8,3:131072,4:65536,5:16,5:4096,6:32768,7:512,8:256,9:32"
    );
    // One player: no controller-2 bindings.
    assert_eq!(b.pad_env_value_p2("ford").unwrap(), "");
    assert_eq!(b.pad_key_env_value_p2("ford").unwrap(), "");
}
