//! Shared controls behaviour against the registered built-in games.
use banana_input::controls::*;

fn setup() {
    banana_compat::install();
}

#[test]
fn defaults_are_valid_and_match_previous_fixed_keys() {
    setup();
    let b = Bindings::default();
    b.validate().unwrap();
    assert_eq!(b.codes("aliens").unwrap()[0], (0x154, 49));
    assert!(b.codes("aliens").unwrap().contains(&(0x150, 116)));
    let jurassic = b.env_value("jurassic").unwrap();
    assert!(jurassic.starts_with("371:49,372:50,379:53,380:54,378:116,377:57,383:273,384:274,385:13,386:8,333:326,334:327"));
    assert_eq!(b.codes("big_buck").unwrap()[..2], [(5, 49), (5, 13)]);
    // Super Cars must match the hook DLL's built-in fallback bindings.
    let cars = b.env_value("super_cars").unwrap();
    assert!(cars.starts_with("1:203,2:205,3:200,4:208,65536:28,65536:2,65537:46"));
    assert!(cars.ends_with("65547:20,65548:10,65550:201,65551:209"));
    // The Linux shim's controller fallbacks (jamma.rs, jurassic_pad.rs,
    // aliens_exit.rs) must match too.
    assert_eq!(
        b.pad_env_value("big_buck").unwrap(),
        "5:16,7:32,256:131072,256:4096,257:65536,257:8192"
    );
    assert_eq!(
        b.pad_env_value("jurassic").unwrap(),
        "371:16,379:32,383:1,384:2,385:4096,386:8192,333:131072,334:65536"
    );
    assert_eq!(
        b.pad_key_env_value("aliens").unwrap(),
        "49:16,49:4096,53:32,57:8192,273:1,274:2"
    );
    // Ford's adapter has the same controller fallback (DEFAULT_PAD).
    assert_eq!(
        b.pad_env_value("ford").unwrap(),
        "1:4,2:8,3:131072,4:65536,5:16,5:4096,6:32768,7:512,8:256,9:32"
    );
    // ... and its built-in controller fallback (DEFAULT_PAD in hook.rs).
    assert_eq!(
        b.pad_env_value("super_cars").unwrap(),
        "1:4,2:8,3:131072,4:65536,65536:16,65536:4096,65537:16384,65538:32768,\
65539:8192,65540:128,65541:256,65542:512,65543:1,65544:2,65545:32"
    );
}

#[test]
fn controller_two_drives_player_two() {
    setup();
    let b = Bindings::default();
    // The Linux shims' DEFAULT_PAD_P2 fallbacks must match these.
    assert_eq!(
        b.pad_env_value_p2("big_buck").unwrap(),
        "6:16,8:32,258:131072,258:4096,259:65536,259:8192"
    );
    assert_eq!(
        b.pad_env_value_p2("jurassic").unwrap(),
        "372:16,380:32,383:1,384:2,385:4096,386:8192,343:131072,344:65536"
    );
    assert_eq!(
        b.pad_key_env_value_p2("aliens").unwrap(),
        "50:16,50:4096,54:32,57:8192,273:1,274:2"
    );
    // One-player games have no controller-2 bindings.
    assert_eq!(b.pad_env_value_p2("ford").unwrap(), "");
    assert_eq!(b.pad_env_value_p2("super_cars").unwrap(), "");
    assert_eq!(b.pad_key_env_value_p2("ford").unwrap(), "");
    // Every listed player-one action exists.
    for g in games() {
        for (id, _) in g.player_two {
            assert!(g.actions.iter().any(|a| a.id == *id), "{} {id}", g.id);
        }
    }
    // Rebinding player one's start moves controller 2's start with it.
    let mut b = b;
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

