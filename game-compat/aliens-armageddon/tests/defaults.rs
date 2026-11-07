//! Aliens Armageddon's default settings and bindings. The adapter's built-in fallbacks
//! (used when no environment value is passed) must match these.
use banana_api::GameCompat;
use banana_game_aliens_armageddon::GAME;
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
    assert_eq!(b.codes("aliens").unwrap()[0], (0x154, 49));
    assert!(b.codes("aliens").unwrap().contains(&(0x150, 116)));
    // aliens_exit.rs controller fallbacks.
    assert_eq!(
        b.pad_key_env_value("aliens").unwrap(),
        "49:16,49:4096,53:32,57:8192,273:1,274:2"
    );
    assert_eq!(
        b.pad_key_env_value_p2("aliens").unwrap(),
        "50:16,50:4096,54:32,57:8192,273:1,274:2"
    );
    let s = banana_settings::Settings::default();
    assert_eq!(s.game("aliens").resolution_text(), "1360x768");
}
