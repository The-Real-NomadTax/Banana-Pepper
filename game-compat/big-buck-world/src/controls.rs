//! Cabinet actions and default bindings, rendered by the shared controls UI.
#[allow(unused_imports)]
use banana_input::controls::{Game, KeySpace, MovementSpec, any, one, pad};

pub static CONTROLS: Game = Game {
    id: "big_buck_world",
    title: "Big Buck World",
    keys: KeySpace::Glut,
    mouse_buttons: false,
    note: "Left mouse fires and right mouse pumps for the keyboard-and-mouse player. Letters, digits, Space, Enter, Tab, Backspace and punctuation only. Controllers: either stick moves the crosshair. Mice and light guns on Raw Input aim their own gun directly.",
    movement: Some(MovementSpec::AIM),
    player_two: &[
        ("start1", 6),
        ("coin1", 8),
        ("trigger", 2),
        ("fire", 0x102),
        ("pump", 0x103),
    ],
    actions: &[
        any("start1", "Player 1 Start", 5, &["1", "Enter", "PadStart"]),
        any("start2", "Player 2 Start", 6, &["2"]),
        any("coin1", "Coin 1", 7, &["5", "PadBack"]),
        any("coin2", "Coin 2", 8, &["6"]),
        any("test", "Test (operator menu)", 11, &["T"]),
        any("service", "Service", 10, &["9"]),
        any("trigger", "Trigger switch (keyboard)", 1, &["Space"]),
        pad(
            "fire",
            "Fire at the crosshair (controller)",
            0x100,
            &["PadRT", "PadA"],
        ),
        pad("pump", "Pump (controller)", 0x101, &["PadLT", "PadB"]),
    ],
};
