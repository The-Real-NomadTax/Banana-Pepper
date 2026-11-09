//! Big Buck World's default settings and bindings. The adapter's built-in fallbacks
//! (used when no environment value is passed) must match these.
use banana_api::GameCompat;
use banana_game_big_buck_world::GAME;
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
    assert_eq!(b.codes("big_buck_world").unwrap()[..2], [(5, 49), (5, 13)]);
    // jamma.rs DEFAULT_PAD / DEFAULT_PAD_P2.
    assert_eq!(
        b.pad_env_value("big_buck_world").unwrap(),
        "5:16,7:32,256:131072,256:4096,257:65536,257:8192"
    );
    assert_eq!(
        b.pad_env_value_p2("big_buck_world").unwrap(),
        "6:16,8:32,258:131072,258:4096,259:65536,259:8192"
    );
    let s = banana_settings::Settings::default();
    let bbh = s.game("big_buck_world");
    assert_eq!(bbh.resolution, (640, 480));
    assert_eq!(
        bbh.renderer,
        if cfg!(windows) {
            banana_settings::Renderer::Gpu
        } else {
            banana_settings::Renderer::Software
        }
    );
    assert!(!bbh.fullscreen && bbh.audio);
    assert_eq!(bbh.crosshair.id(), "p1");
    assert!(bbh.guest_env().contains(&("ARCADE_CROSSHAIR", "p1".into())));
}
