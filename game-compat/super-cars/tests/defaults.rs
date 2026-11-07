//! Super Cars's default settings and bindings. The adapter's built-in fallbacks
//! (used when no environment value is passed) must match these.
use banana_api::GameCompat;
use banana_game_super_cars::GAME;
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
    // hook.rs built-in key bindings and DEFAULT_PAD.
    let cars = b.env_value("super_cars").unwrap();
    assert!(cars.starts_with("1:203,2:205,3:200,4:208,65536:28,65536:2,65537:46"));
    assert!(cars.ends_with("65547:20,65548:10,65550:201,65551:209"));
    assert_eq!(
        b.pad_env_value("super_cars").unwrap(),
        "1:4,2:8,3:131072,4:65536,65536:16,65536:4096,65537:16384,65538:32768,65539:8192,65540:128,65541:256,65542:512,65543:1,65544:2,65545:32"
    );
    // One player: no controller-2 bindings.
    assert_eq!(b.pad_env_value_p2("super_cars").unwrap(), "");
}
