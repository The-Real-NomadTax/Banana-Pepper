//! Jurassic Park's default settings and bindings. The adapter's built-in fallbacks
//! (used when no environment value is passed) must match these.
use banana_api::GameCompat;
use banana_game_jurassic_park::GAME;
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
    assert!(b.env_value("jurassic").unwrap().starts_with(
        "371:49,372:50,379:53,380:54,378:116,377:57,383:273,384:274,385:13,386:8,333:326,334:327"
    ));
    // jurassic_pad.rs DEFAULT_PAD / DEFAULT_PAD_P2.
    assert_eq!(
        b.pad_env_value("jurassic").unwrap(),
        "371:16,379:32,383:1,384:2,385:4096,386:8192,333:131072,334:65536"
    );
    assert_eq!(
        b.pad_env_value_p2("jurassic").unwrap(),
        "372:16,380:32,383:1,384:2,385:4096,386:8192,343:131072,344:65536"
    );
    let s = banana_settings::Settings::default();
    assert_eq!(
        s.game("jurassic").guest_env(),
        [("ARCADE_RESOLUTION", "1360x768".into())]
    );
}
