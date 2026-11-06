//! Cabinet actions and default bindings, rendered by the shared controls UI.
#[allow(unused_imports)]
use banana_input::controls::{any, one, pad, Game, KeySpace};

pub static CONTROLS: Game = Game {
        id: "aliens",
        title: "Aliens Armageddon",
        keys: KeySpace::Sdl,
        mouse_buttons: false,
        note: "Mouse aim and fire use the game's own desktop mouse controls. One key per action (plus any controller buttons). Controllers: either stick moves the pointer, RT = left click, LT = right click. Two players: controller 2 aims player two, RT/LT = Q/W (the game's player-two fire keys).",
        player_two: &[("start1", 0x155), ("coin1", 0x14d)],
        actions: &[
            one("start1", "Player 1 Start / menu confirm", 0x154, &["1", "PadStart", "PadA"]),
            one("start2", "Player 2 Start", 0x155, &["2"]),
            one("coin1", "Coin 1", 0x14c, &["5", "PadBack"]),
            one("coin2", "Coin 2", 0x14d, &["6"]),
            one("test", "Test (operator menu) / menu select", 0x150, &["T"]),
            one("service", "Service / menu back", 0x151, &["9", "PadB"]),
            one("volume_up", "Volume up / menu up", 0x152, &["Up", "PadUp"]),
            one("volume_down", "Volume down / menu down", 0x153, &["Down", "PadDown"]),
        ],
    };
