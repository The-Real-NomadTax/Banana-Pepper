//! Cabinet actions and default bindings, rendered by the shared controls UI.
#[allow(unused_imports)]
use banana_input::controls::{any, one, pad, Game, KeySpace, MovementSpec};

pub static CONTROLS: Game = Game {
        id: "jurassic",
        title: "Jurassic Park 2015",
        keys: KeySpace::Sdl,
        mouse_buttons: true,
        note: "The mouse aims for the keyboard-and-mouse player (player one unless player two picks it). Controllers: either stick moves the aim. Mice and light guns on Raw Input aim their own player directly.",
        movement: Some(MovementSpec::AIM),
        player_two: &[("start1", 372), ("coin1", 380), ("fire", 343), ("secondary", 344)],
        actions: &[
            any("start1", "Player 1 Start", 371, &["1", "PadStart"]),
            any("start2", "Player 2 Start", 372, &["2"]),
            any("coin1", "Coin 1", 379, &["5", "PadBack"]),
            any("coin2", "Coin 2", 380, &["6"]),
            any("test", "Test (operator menu)", 378, &["T"]),
            any("service", "Service", 377, &["9"]),
            any("menu_up", "Menu up", 383, &["Up", "PadUp"]),
            any("menu_down", "Menu down", 384, &["Down", "PadDown"]),
            any("menu_select", "Menu select", 385, &["Enter", "PadA"]),
            any("menu_back", "Menu back", 386, &["Backspace", "PadB"]),
            any("fire", "Fire (player one)", 333, &["MouseLeft", "PadRT"]),
            any("secondary", "Secondary trigger", 334, &["MouseRight", "PadLT"]),
        ],
    };
