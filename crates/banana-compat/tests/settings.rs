//! Shared settings behaviour against the registered built-in games.
use banana_settings::*;

fn setup() {
    banana_compat::install();
}

#[test]
fn crosshair_setting_only_for_gun_games() {
    setup();
    let s = Settings::parse("[big_buck]\ncrosshair = P2_LARGE\n").unwrap();
    assert_eq!(s.game("big_buck").crosshair.label(), "Blue, large (P2)");
    assert_eq!(Settings::parse(&s.to_text()).unwrap(), s);
    let off = Settings::parse("[big_buck]\ncrosshair = off\n").unwrap();
    assert!(
        !off.game("big_buck")
            .guest_env()
            .iter()
            .any(|e| e.0 == "ARCADE_CROSSHAIR")
    );
    assert!(Settings::parse("[big_buck]\ncrosshair = green\n").is_err());
    assert!(Settings::parse("[aliens]\ncrosshair = p1\n").is_err());
    assert!(Settings::parse("[aliens]\ncrosshair_size = 50\n").is_err());
    let sized = Settings::parse("[big_buck]\ncrosshair_size = 35%\n").unwrap();
    assert_eq!(sized.game("big_buck").crosshair_size, 35);
    assert!(
        sized
            .game("big_buck")
            .guest_env()
            .contains(&("ARCADE_CROSSHAIR_SIZE", "35".into()))
    );
    assert_eq!(Settings::parse(&sized.to_text()).unwrap(), sized);
    assert!(Settings::parse("[big_buck]\ncrosshair_size = 5\n").is_err());
    assert!(Settings::parse("[big_buck]\ncrosshair_size = huge\n").is_err());
    // Only Big Buck writes a crosshair line.
    assert_eq!(
        Settings::default().to_text().matches("crosshair =").count(),
        1
    );
    assert_eq!(
        Settings::default()
            .to_text()
            .matches("crosshair_size =")
            .count(),
        1
    );
    assert_eq!(Crosshair::all().len(), 5);
}

#[test]
fn two_players_only_for_two_player_games() {
    setup();
    let s = Settings::parse("[jurassic]\nplayers = 2\n[big_buck]\nplayers = 2\n").unwrap();
    assert_eq!(s.game("jurassic").players, 2);
    assert!(
        s.game("jurassic")
            .guest_env()
            .contains(&("ARCADE_PLAYERS", "2".into()))
    );
    assert!(
        s.game("jurassic")
            .summary()
            .ends_with("2 players (P1 XInput, P2 XInput)")
    );
    assert_eq!(Settings::parse(&s.to_text()).unwrap(), s);
    // One player (the default) sets nothing for the guest.
    let one = Settings::default();
    assert_eq!(one.game("aliens").players, 1);
    assert!(
        !one.game("aliens")
            .guest_env()
            .iter()
            .any(|e| e.0 == "ARCADE_PLAYERS")
    );
    assert!(Settings::parse("[aliens]\nplayers = 4\n").is_err());
    assert!(Settings::parse("[ford]\nplayers = 2\n").is_err());
    assert!(Settings::parse("[super_cars]\nplayers = 1\n").is_ok());
    // Aliens, Jurassic and Big Buck write a players line.
    assert_eq!(one.to_text().matches("players =").count(), 3);
}

#[test]
fn players_choose_their_input_method() {
    setup();
    let s =
        Settings::parse("[big_buck]\nplayers = 2\np1_input = dinput\np2_input = keyboard_mouse\n")
            .unwrap();
    let g = s.game("big_buck");
    assert_eq!(
        g.inputs,
        [InputMethod::DirectInput, InputMethod::KeyboardMouse]
    );
    assert!(
        g.guest_env()
            .contains(&("ARCADE_INPUTS", "dinput,keyboard_mouse".into()))
    );
    assert!(
        g.summary()
            .ends_with("2 players (P1 DirectInput, P2 Keyboard & mouse)")
    );
    assert_eq!(Settings::parse(&s.to_text()).unwrap(), s);
    // The older words still load.
    let old = Settings::parse(
        "[jurassic]\nplayers = 2\np1_input = controller\np2_input = keyboard_mouse\n",
    )
    .unwrap();
    assert_eq!(
        old.game("jurassic").inputs,
        [InputMethod::XInput, InputMethod::KeyboardMouse]
    );
    // Only one player can have the keyboard and mouse.
    let s = Settings::parse(
        "[jurassic]\nplayers = 2\np2_input = keyboard_mouse\np1_input = keyboard_mouse\n",
    )
    .unwrap();
    assert_eq!(
        s.game("jurassic").inputs,
        [InputMethod::KeyboardMouse, InputMethod::XInput]
    );
    // Two light guns.
    let guns = Settings::parse("[aliens]\nplayers = 2\np1_input = rawinput\np2_input = rawinput\n")
        .unwrap();
    assert_eq!(guns.game("aliens").input_assignment().mouse(), None);
    // Player one's method applies with one player too; the default sets nothing.
    let solo = Settings::parse("[aliens]\np1_input = rawinput\n").unwrap();
    assert!(
        solo.game("aliens")
            .guest_env()
            .contains(&("ARCADE_INPUTS", "rawinput,xinput".into()))
    );
    assert!(
        !Settings::default()
            .game("aliens")
            .guest_env()
            .iter()
            .any(|e| e.0 == "ARCADE_INPUTS")
    );
    // Racers: one player, keyboard / XInput / DirectInput (wheels).
    let wheel = Settings::parse("[super_cars]\np1_input = dinput\n").unwrap();
    assert!(
        wheel
            .game("super_cars")
            .guest_env()
            .contains(&("ARCADE_INPUTS", "dinput,xinput".into()))
    );
    assert!(Settings::parse("[ford]\np1_input = rawinput\n").is_err());
    assert!(Settings::parse("[ford]\np2_input = xinput\n").is_err());
    assert!(Settings::parse("[aliens]\np1_input = gamepad\n").is_err());
    // One-player games (racers, Skylanders) write only p1_input; gun games write both.
    let text = Settings::default().to_text();
    assert_eq!(text.matches("p1_input =").count(), 7);
    assert_eq!(text.matches("p2_input =").count(), 3);
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

#[test]
fn gun_aim_modes_round_trip_and_reach_the_guest_per_player() {
    setup();
    let s = Settings::parse("[jurassic]\nplayers = 2\np1_aim = centered\np2_aim = cursor\n[aliens]\np2_aim = centered\n").unwrap();
    assert_eq!(
        s.game("jurassic").aim_modes,
        [AimMode::Centered, AimMode::Cursor]
    );
    assert!(
        s.game("jurassic")
            .guest_env()
            .contains(&("ARCADE_AIM_MODES", "centered,cursor".into()))
    );
    assert!(
        s.game("aliens")
            .guest_env()
            .contains(&("ARCADE_AIM_MODES", "cursor,centered".into()))
    );
    assert_eq!(Settings::parse(&s.to_text()).unwrap(), s);
    assert_eq!(
        Settings::default().game("big_buck").aim_modes,
        [AimMode::Cursor; 2]
    );
    assert!(Settings::parse("[jurassic]\np1_aim = invalid\n").is_err());
    assert!(Settings::parse("[super_cars]\np1_aim = centered\n").is_err());
}

#[test]
fn operator_binding_input_is_saved_independently_of_players() {
    setup();
    for (id, method) in [
        ("keyboard_mouse", InputMethod::KeyboardMouse),
        ("xinput", InputMethod::XInput),
        ("dinput", InputMethod::DirectInput),
        ("rawinput", InputMethod::RawInput),
    ] {
        let s = Settings::parse(&format!("[jurassic]\noperator_input = {id}\n")).unwrap();
        assert_eq!(s.game("jurassic").operator_input, method);
        assert_eq!(s.game("jurassic").inputs, [InputMethod::XInput; 2]);
        assert_eq!(Settings::parse(&s.to_text()).unwrap(), s);
    }
    assert!(Settings::parse("[jurassic]\noperator_input = invalid\n").is_err());
}

#[test]
fn legacy_backend_selection_migrates_to_water() {
    setup();
    let temp = std::env::temp_dir().join(format!(
        "banana-backend-migration-{}.cfg",
        std::process::id()
    ));
    std::fs::write(&temp, "[big_buck]\nbackend = wsl\n").unwrap();
    let settings = Settings::load(&temp).unwrap();
    assert_eq!(settings.game("big_buck").backend, Backend::Water);
    assert!(!settings.to_text().contains("backend = wsl"));
    std::fs::remove_file(temp).unwrap();
}
