//! Cabinet actions and default bindings, rendered by the shared controls UI.
//! Targets are read by the touch adapter (`hook/src/hook.rs`).
#[allow(unused_imports)]
use banana_input::controls::{any, one, pad, Game, KeySpace, MovementSpec};

pub static CONTROLS: Game = Game {
    id: "skylanders",
    title: "Skylanders Cloud Patrol",
    keys: KeySpace::DirectInput,
    mouse_buttons: false,
    note: "The game is played on a touch screen: the left mouse button always touches at the cursor. The keys and controller buttons below touch at the cursor too, and the controller's aim axes move the cursor. A game costs 4 coins by default. Escape (or Back + Start) exits.",
    movement: Some(MovementSpec::AIM),
    player_two: &[],
    actions: &[
        any("touch", "Touch / shoot at the cursor", 1, &["Space", "PadA", "PadRT"]),
        any("coin", "Insert coin", 2, &["5", "PadBack"]),
    ],
};
