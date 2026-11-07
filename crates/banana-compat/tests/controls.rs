//! Shared controls behaviour against the registered built-in games.
use banana_input::controls::*;

fn setup() {
    banana_compat::install();
}

// Each game's default bindings are checked against its adapter's built-in
// fallbacks in game-compat/<game>/tests/defaults.rs.
#[test]
fn every_registered_default_is_valid() {
    setup();
    Bindings::default().validate().unwrap();
    // Every listed player-two action exists for player one.
    for g in games() {
        for (id, _) in g.player_two {
            assert!(g.actions.iter().any(|a| a.id == *id), "{} {id}", g.id);
        }
    }
}

#[test]
fn rebinding_player_one_start_moves_controller_two() {
    setup();
    let mut b = Bindings::default();
    b.toggle_pad("big_buck", "start1", "PadY").unwrap();
    assert!(b.pad_env_value_p2("big_buck").unwrap().contains("6:32768"));
}

#[test]
fn player_two_has_its_own_controller_layout() {
    setup();
    let mut b = Bindings::default();
    assert!(!b.p2_custom("jurassic"));
    // Player two fires with A instead of RT; A leaves menu select.
    b.toggle_pad_p2("jurassic", "fire", "PadRT").unwrap();
    b.toggle_pad_p2("jurassic", "fire", "PadA").unwrap();
    assert!(b.p2_custom("jurassic"));
    assert_eq!(b.p2_pads("jurassic", "fire"), ["PadA"]);
    assert!(b.p2_pads("jurassic", "menu_select").is_empty());
    let p2 = b.pad_env_value_p2("jurassic").unwrap();
    assert!(p2.contains("343:4096") && !p2.contains("343:131072") && !p2.contains("385:"));
    // Player one is untouched and stops being mirrored.
    assert_eq!(b.keys("jurassic", "fire"), ["MouseLeft", "PadRT"]);
    b.toggle_pad("jurassic", "coin1", "PadY").unwrap();
    assert!(!b.pad_env_value_p2("jurassic").unwrap().contains("380:32768"));
    // Saved as a [jurassic.p2] section and read back unchanged.
    let text = b.to_text();
    assert_eq!(text.matches("[jurassic.p2]").count(), 1);
    assert_eq!(Bindings::parse(&text).unwrap(), b);
    // Resetting player two mirrors player one again.
    b.reset_p2("jurassic");
    assert!(!b.p2_custom("jurassic"));
    assert!(!b.to_text().contains(".p2]"));
    assert!(b.pad_env_value_p2("jurassic").unwrap().contains("380:32768"));
    // Only two-player games, and only controller buttons.
    assert!(b.toggle_pad_p2("ford", "gas", "PadA").is_err());
    assert!(Bindings::parse("[ford.p2]\ngas = PadA\n").is_err());
    assert!(Bindings::parse("[big_buck.p2]\nfire = Space\n").is_err());
    assert!(Bindings::parse("[big_buck.p2]\nfire = PadA\npump = PadA\n").is_err());
    let custom = Bindings::parse("[big_buck.p2]\nfire = PadRB\n").unwrap();
    assert_eq!(custom.pad_env_value_p2("big_buck").unwrap(), "258:512");
}

#[test]
fn player_two_menu_roles_and_own_keys() {
    setup();
    let role = |g: &str, a: &str| {
        let g = game(g).unwrap();
        player_two_role(g, g.actions.iter().find(|x| x.id == a).unwrap())
    };
    assert!(matches!(role("jurassic", "start1"), PlayerTwo::Linked(a) if a.id == "start2"));
    assert!(matches!(role("jurassic", "coin1"), PlayerTwo::Linked(a) if a.id == "coin2"));
    assert!(matches!(role("jurassic", "start2"), PlayerTwo::Hidden));
    assert!(matches!(role("jurassic", "fire"), PlayerTwo::Own(343)));
    assert!(matches!(role("jurassic", "test"), PlayerTwo::Shared));
    assert!(matches!(role("big_buck", "trigger"), PlayerTwo::Own(2)));
    assert!(matches!(role("aliens", "start1"), PlayerTwo::Linked(a) if a.id == "start2"));

    let mut b = Bindings::default();
    // Player two fires with K; it reaches the game on player two's target.
    b.rebind_p2("jurassic", "fire", "K", false).unwrap();
    assert_eq!(b.p2_keys("jurassic", "fire"), ["K"]);
    assert!(b.codes("jurassic").unwrap().contains(&(343, 107)));
    // Player two's controller buttons are kept (copied from player one).
    assert_eq!(b.p2_pads("jurassic", "fire"), ["PadRT"]);
    // Keys player one uses, mouse buttons and shared actions are refused.
    assert!(b.rebind_p2("jurassic", "secondary", "T", false).is_err());
    assert!(b.rebind_p2("jurassic", "secondary", "MouseRight", false).is_err());
    assert!(b.rebind_p2("jurassic", "test", "L", false).is_err());
    assert!(b.rebind_p2("big_buck", "fire", "L", false).is_err()); // controller only
    // ...and player one cannot take player two's key.
    assert!(b.rebind("jurassic", "test", "K", false).is_err());
    // Saved and read back.
    let text = b.to_text();
    assert!(text.contains("fire = K, PadRT"));
    assert_eq!(Bindings::parse(&text).unwrap(), b);
    b.clear_p2_keys("jurassic", "fire");
    assert!(b.p2_keys("jurassic", "fire").is_empty());
    // Big Buck's keyboard trigger for gun two.
    b.rebind_p2("big_buck", "trigger", "L", false).unwrap();
    assert!(b.codes("big_buck").unwrap().contains(&(2, 108)));
    assert!(Bindings::parse("[jurassic.p2]\ntest = L\n").is_err());
}

#[test]
fn round_trip_and_overrides() {
    setup();
    let mut b = Bindings::default();
    b.set("aliens", "test", &["F2"]).unwrap();
    b.set("jurassic", "fire", &["MouseLeft", "Space"]).unwrap();
    let parsed = Bindings::parse(&b.to_text()).unwrap();
    assert_eq!(parsed, b);
    let p = Bindings::parse("[aliens]\ntest = f9 # comment\n").unwrap();
    assert_eq!(p.codes("aliens").unwrap()[4], (0x150, 290));
    assert_eq!(p.keys("aliens", "start1"), ["1", "PadStart", "PadA"]);
}

#[test]
fn rebinding_moves_or_swaps_conflicting_keys() {
    setup();
    let mut b = Bindings::default();
    // Aliens: T to start1 swaps start1's old key "1" onto test.
    b.rebind("aliens", "start1", "T", false).unwrap();
    assert_eq!(b.keys("aliens", "start1"), ["T", "PadStart", "PadA"]);
    assert_eq!(b.keys("aliens", "test"), ["1"]);
    b.validate().unwrap();
    // Jurassic: adding Space to fire keeps the mouse button.
    b.rebind("jurassic", "fire", "Space", true).unwrap();
    assert_eq!(b.keys("jurassic", "fire"), ["MouseLeft", "Space", "PadRT"]);
    // Big Buck: Enter moves from start1 (which keeps "1").
    b.rebind("big_buck", "test", "Enter", false).unwrap();
    assert_eq!(b.keys("big_buck", "start1"), ["1", "PadStart"]);
    assert_eq!(b.keys("big_buck", "test"), ["Enter"]);
    // Invalid keys change nothing.
    let before = b.clone();
    assert!(b.rebind("aliens", "test", "MouseLeft", false).is_err());
    assert!(b.rebind("aliens", "test", "Escape", false).is_err());
    assert!(b.rebind("aliens", "test", "Z", true).is_err()); // single-key action
    assert!(b.clear("aliens", "test").is_err());
    assert_eq!(b, before);
    b.clear("jurassic", "secondary").unwrap();
    assert!(b.keys("jurassic", "secondary").is_empty());
}

#[test]
fn controller_buttons_are_separate_from_keys() {
    setup();
    let b = Bindings::default();
    // Keyboard codes are unchanged by the controller defaults.
    assert_eq!(b.codes("aliens").unwrap().len(), 8);
    let pads = b.pad_codes("super_cars").unwrap();
    assert!(pads.contains(&(3, PAD_RT)));
    assert!(pads.contains(&(0x10000, 0x10)));
    assert!(b.pad_env_value("jurassic").unwrap().contains("333:131072"));
    // Aliens presses each action's own key: start1 "1" (49) for Start and A.
    assert!(b.pad_key_env_value("aliens").unwrap().starts_with("49:16,49:4096,"));
    assert_eq!(b.pad_codes("big_buck").unwrap()[..1], [(5, 0x10)]);
    // Pad-only actions refuse keyboard keys; keys are not pads.
    let mut c = b.clone();
    assert!(c.set("big_buck", "fire", &["Space"]).is_err());
    assert!(c.set("big_buck", "fire", &["PadX"]).is_ok());
    assert_eq!(normalize("pad_a".replace('_', "").as_str()).as_deref(), Some("PadA"));
    assert_eq!(normalize("PadView").as_deref(), Some("PadBack"));
}

#[test]
fn toggling_and_rebinding_keep_controller_buttons() {
    setup();
    let mut b = Bindings::default();
    // Replacing a key keeps the controller buttons.
    b.rebind("aliens", "start1", "F1", false).unwrap();
    assert_eq!(b.keys("aliens", "start1"), ["F1", "PadStart", "PadA"]);
    // Moving a single action's only key swaps, even with pads left over.
    b.rebind("aliens", "start1", "Up", false).unwrap();
    assert_eq!(b.keys("aliens", "volume_up"), ["F1", "PadUp"]);
    b.validate().unwrap();
    // Toggling a pad moves it from another action, then removes it.
    b.toggle_pad("aliens", "test", "PadA").unwrap();
    assert!(!b.keys("aliens", "start1").contains(&"PadA".to_owned()));
    assert!(b.keys("aliens", "test").contains(&"PadA".to_owned()));
    b.toggle_pad("aliens", "test", "PadA").unwrap();
    assert!(!b.keys("aliens", "test").contains(&"PadA".to_owned()));
    assert!(b.toggle_pad("aliens", "test", "Space").is_err());
    let parsed = Bindings::parse(&b.to_text()).unwrap();
    assert_eq!(parsed, b);
}

#[test]
fn normalizes_egui_and_user_spellings() {
    setup();
    assert_eq!(normalize("Num7").as_deref(), Some("7"));
    assert_eq!(normalize("ArrowUp").as_deref(), Some("Up"));
    assert_eq!(normalize("t").as_deref(), Some("T"));
    assert_eq!(normalize("OpenBracket").as_deref(), Some("LeftBracket"));
    assert_eq!(normalize("pageup").as_deref(), Some("PageUp"));
    assert_eq!(normalize("Keypad4").as_deref(), Some("Keypad4"));
    assert_eq!(normalize("bogus"), None);
}

#[test]
fn rejects_bad_bindings() {
    setup();
    assert!(Bindings::parse("[aliens]\ntest = 1\n").is_err()); // duplicate of start1
    assert!(Bindings::parse("[aliens]\ntest = T, Y\n").is_err()); // single-key action
    assert!(Bindings::parse("[aliens]\ntest =\n").is_err());
    assert!(Bindings::parse("[aliens]\ntest = Escape\n").is_err());
    assert!(Bindings::parse("[aliens]\ntest = MouseLeft\n").is_err());
    assert!(Bindings::parse("[big_buck]\ntest = Up\n").is_err()); // GLUT has no arrows
    assert!(Bindings::parse("[nope]\n").is_err());
    assert!(Bindings::parse("test = T\n").is_err());
    // Jurassic actions may be unbound.
    Bindings::parse("[jurassic]\nsecondary =\n").unwrap();
}


#[test]
fn buttons_are_named_for_each_input_method() {
    setup();
    use banana_input::pad::Method;
    assert_eq!(pad_label(Method::XInput, "PadRT"), Some("Right trigger"));
    assert_eq!(pad_label(Method::DirectInput, "PadA"), Some("Button 1"));
    assert_eq!(pad_label(Method::DirectInput, "PadRT"), Some("Button 8"));
    assert_eq!(pad_label(Method::DirectInput, "PadUp"), Some("Hat up"));
    assert_eq!(pad_label(Method::RawInput, "PadRT"), Some("Left button"));
    assert_eq!(pad_label(Method::RawInput, "PadStart"), Some("Side button"));
    assert_eq!(pad_label(Method::RawInput, "PadY"), None);
    let mut b = Bindings::default();
    b.rebind_p2("jurassic", "fire", "K", false).unwrap();
    b.rebind("jurassic", "test", "F", false).unwrap();
    b.reset_player_one("jurassic");
    assert_eq!(b.keys("jurassic", "test"), ["T"]);
    assert_eq!(b.p2_keys("jurassic", "fire"), ["K"]);
}

#[test]
fn movement_axes_are_bindable_per_player() {
    setup();
    use banana_input::pad::Method;
    let mut b = Bindings::default();
    // Defaults: either stick aims; the left stick steers with Y unbound.
    assert_eq!(b.move_env_value("jurassic", 0), "x=AxisLX,AxisRX;y=AxisLY,AxisRY");
    assert_eq!(b.move_env_value("super_cars", 0), "x=AxisLX;y=");
    assert_eq!(b.move_env_value("jurassic", 1), b.move_env_value("jurassic", 0));
    // Player one aims with the right stick only, inverted Y.
    b.set_movement("jurassic", 0, 0, &["axisrx".into()]).unwrap();
    b.set_movement("jurassic", 0, 1, &["AxisRY-".into()]).unwrap();
    assert_eq!(b.move_env_value("jurassic", 0), "x=AxisRX;y=AxisRY-");
    // Player two still mirrors player one until it gets its own.
    assert_eq!(b.move_env_value("jurassic", 1), "x=AxisRX;y=AxisRY-");
    b.set_movement("jurassic", 1, 0, &["PointerY".into()]).unwrap();
    assert!(b.p2_custom("jurassic"));
    assert_eq!(b.move_env_value("jurassic", 1), "x=PointerY;y=AxisRY-");
    // A racer's gas/brake on a pedal axis.
    b.set_movement("super_cars", 0, 1, &["AxisRY".into()]).unwrap();
    assert_eq!(b.move_env_value("super_cars", 0), "x=AxisLX;y=AxisRY");
    // Saved and read back.
    let text = b.to_text();
    assert!(text.contains("move_x = AxisRX  #") && text.contains("move_x = PointerY  #"));
    assert_eq!(Bindings::parse(&text).unwrap(), b);
    // Refused: unknown axes, too many, games without movement, P2 on a racer.
    assert!(b.set_movement("jurassic", 0, 0, &["Wheel".into()]).is_err());
    assert!(b.set_movement("jurassic", 0, 0, &["AxisLX".into(), "AxisRX".into(), "AxisLT".into()]).is_err());
    assert!(b.set_movement("ford", 1, 0, &["AxisLX".into()]).is_err());
    assert!(Bindings::parse("[jurassic]\nmove_x = Wheel\n").is_err());
    // Resetting player one restores the default axes.
    b.reset_player_one("jurassic");
    assert_eq!(b.move_env_value("jurassic", 0), "x=AxisLX,AxisRX;y=AxisLY,AxisRY");
    // Names per method.
    assert_eq!(axis_label(Method::XInput, "AxisLY").as_deref(), Some("Left stick Y"));
    assert_eq!(axis_label(Method::DirectInput, "AxisRX").as_deref(), Some("Z axis"));
    assert_eq!(axis_label(Method::DirectInput, "AxisLX-").as_deref(), Some("X axis (inverted)"));
    assert_eq!(axis_label(Method::RawInput, "PointerX").as_deref(), Some("Pointer X"));
    assert_eq!(axis_label(Method::RawInput, "AxisLX"), None);
    assert_eq!(axis_label(Method::XInput, "PointerX"), None);
}
