//! Cabinet actions and default bindings, rendered by the shared controls UI.
#[allow(unused_imports)]
use banana_input::controls::{any, one, pad, Game, KeySpace, MovementSpec};

pub static CONTROLS: Game = Game {
        id: "super_cars",
        title: "Fast and Furious Super Cars",
        keys: KeySpace::DirectInput,
        mouse_buttons: false,
        note: "Steering ramps while held and recentres on release; gas and brake keys are on/off. Controllers (XInput): left stick steers, triggers bound to gas/brake are analog. Targets are the cabinet I/O board's input codes.",
        movement: Some(MovementSpec::STEER),
        player_two: &[],
        actions: &[
            any("steer_left", "Steer left", 1, &["Left", "PadLeft"]),
            any("steer_right", "Steer right", 2, &["Right", "PadRight"]),
            any("gas", "Gas", 3, &["Up", "PadRT"]),
            any("brake", "Brake", 4, &["Down", "PadLT"]),
            any("start1", "Start / select", 0x10000, &["Enter", "1", "PadStart", "PadA"]),
            any("view1", "View 1", 0x10001, &["C", "PadX"]),
            any("view2", "View 2", 0x10002, &["V", "PadY"]),
            any("view3", "View 3", 0x10003, &["B", "PadB"]),
            any("tunes", "Music (tunes)", 0x10004, &["M", "PadRS"]),
            any("gear1", "Gear 1", 0x10005, &["Q", "PadLB"]),
            any("gear2", "Gear 2", 0x10006, &["W", "PadRB"]),
            any("gear3", "Gear 3", 0x10007, &["E", "PadUp"]),
            any("gear4", "Gear 4", 0x10008, &["R", "PadDown"]),
            any("coin1", "Coin 1", 0x10009, &["5", "PadBack"]),
            any("coin2", "Coin 2", 0x1000a, &["6"]),
            any("test", "Test (operator menu)", 0x1000b, &["T"]),
            any("service", "Service", 0x1000c, &["9"]),
            any("volume_up", "Volume up", 0x1000e, &["PageUp"]),
            any("volume_down", "Volume down", 0x1000f, &["PageDown"]),
        ],
    };
