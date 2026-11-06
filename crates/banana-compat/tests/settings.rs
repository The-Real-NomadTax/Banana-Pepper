//! Shared settings behaviour against the registered built-in games.
use banana_settings::*;

fn setup() {
    banana_compat::install();
}

#[test]
fn defaults_match_previous_fixed_launch_behaviour() {
    setup();
    let s = Settings::default();
    assert_eq!(s.game("aliens").resolution_text(), "1360x768");
    assert_eq!(
        s.game("jurassic").guest_env(),
        [("ARCADE_RESOLUTION", "1360x768".into())]
    );
    let bbh = s.game("big_buck");
    assert_eq!(bbh.resolution, (640, 480));
    assert_eq!(bbh.renderer, if cfg!(windows){Renderer::Gpu}else{Renderer::Software});
    assert!(!bbh.fullscreen && bbh.audio);
    assert_eq!(bbh.crosshair.id(), "p1");
    assert!(bbh.guest_env().contains(&("ARCADE_CROSSHAIR", "p1".into())));
}

#[test]
fn crosshair_setting_only_for_gun_games() {
    setup();
    let s = Settings::parse("[big_buck]\ncrosshair = P2_LARGE\n").unwrap();
    assert_eq!(s.game("big_buck").crosshair.label(), "Blue, large (P2)");
    assert_eq!(Settings::parse(&s.to_text()).unwrap(), s);
    let off = Settings::parse("[big_buck]\ncrosshair = off\n").unwrap();
    assert!(!off.game("big_buck").guest_env().iter().any(|e| e.0 == "ARCADE_CROSSHAIR"));
    assert!(Settings::parse("[big_buck]\ncrosshair = green\n").is_err());
    assert!(Settings::parse("[aliens]\ncrosshair = p1\n").is_err());
    assert!(Settings::parse("[aliens]\ncrosshair_size = 50\n").is_err());
    let sized = Settings::parse("[big_buck]\ncrosshair_size = 35%\n").unwrap();
    assert_eq!(sized.game("big_buck").crosshair_size, 35);
    assert!(sized.game("big_buck").guest_env().contains(&("ARCADE_CROSSHAIR_SIZE", "35".into())));
    assert_eq!(Settings::parse(&sized.to_text()).unwrap(), sized);
    assert!(Settings::parse("[big_buck]\ncrosshair_size = 5\n").is_err());
    assert!(Settings::parse("[big_buck]\ncrosshair_size = huge\n").is_err());
    // Only Big Buck writes a crosshair line.
    assert_eq!(Settings::default().to_text().matches("crosshair =").count(), 1);
    assert_eq!(Settings::default().to_text().matches("crosshair_size =").count(), 1);
    assert_eq!(Crosshair::all().len(), 5);
}

#[test]
fn two_players_only_for_two_player_games() {
    setup();
    let s = Settings::parse("[jurassic]\nplayers = 2\n[big_buck]\nplayers = 2\n").unwrap();
    assert_eq!(s.game("jurassic").players, 2);
    assert!(s.game("jurassic").guest_env().contains(&("ARCADE_PLAYERS", "2".into())));
    assert!(s.game("jurassic").summary().ends_with("2 players"));
    assert_eq!(Settings::parse(&s.to_text()).unwrap(), s);
    // One player (the default) sets nothing for the guest.
    let one = Settings::default();
    assert_eq!(one.game("aliens").players, 1);
    assert!(!one.game("aliens").guest_env().iter().any(|e| e.0 == "ARCADE_PLAYERS"));
    assert!(Settings::parse("[aliens]\nplayers = 4\n").is_err());
    assert!(Settings::parse("[ford]\nplayers = 2\n").is_err());
    assert!(Settings::parse("[super_cars]\nplayers = 1\n").is_ok());
    // Aliens, Jurassic and Big Buck write a players line.
    assert_eq!(one.to_text().matches("players =").count(), 3);
}

#[test]
fn round_trip_and_partial_files() {
    setup();
    let mut s = Settings::default();
    let g = s.game_mut("jurassic").unwrap();
    g.resolution = (1920, 1080);
    g.fullscreen = true;
    g.vsync = VSync::Off;
    g.renderer = Renderer::Software;
    g.audio = false;
    g.audio_sink = "alsa_output.usb".into();
    assert_eq!(Settings::parse(&s.to_text()).unwrap(), s);
    let p = Settings::parse("[aliens]\nresolution = 1280X720 # c\nvsync = ON\n").unwrap();
    assert_eq!(p.game("aliens").resolution, (1280, 720));
    assert_eq!(p.game("aliens").vsync, VSync::On);
    assert_eq!(p.game("big_buck"), Settings::default().game("big_buck"));
    let env = p.game("aliens").guest_env_value();
    assert!(env.contains("vblank_mode=3") && env.contains("ARCADE_VSYNC=1"));
}

#[test]
fn rejects_bad_settings() {
    setup();
    assert!(Settings::parse("[aliens]\nresolution = big\n").is_err());
    assert!(Settings::parse("[aliens]\nresolution = 10x10\n").is_err());
    assert!(Settings::parse("[aliens]\nvsync = sometimes\n").is_err());
    assert!(Settings::parse("[aliens]\nrenderer = vulkan\n").is_err());
    assert!(Settings::parse("[aliens]\nfullscreen = maybe\n").is_err());
    assert!(Settings::parse("[aliens]\naudio_sink = a b\n").is_err());
    assert!(Settings::parse("[aliens]\nbogus = 1\n").is_err());
    assert!(Settings::parse("[nope]\n").is_err());
    assert!(Settings::parse("audio = true\n").is_err());
}

