//! Cabinet actions and default bindings, rendered by the shared controls UI.
//!
//! Targets are read by the hook's JVS board (`hook/src/board.rs`): 1-4 are
//! the analog wheel and pedals, 0x100+ are switches, 0x200+ the shifter.
use banana_input::controls::{any, Game, KeySpace, MovementSpec};

pub static CONTROLS: Game = Game {
    id: "wmmt6r",
    title: "Wangan Midnight Maximum Tune 6R",
    keys: KeySpace::DirectInput,
    mouse_buttons: false,
    note: "Steering keys ramp like a wheel and recentre on release; the left stick steers directly and triggers bound to gas/brake are analog. The six-speed shifter follows the gear keys (up/down step through neutral). Escape or Back + Start exits.",
    movement: Some(MovementSpec::STEER),
    player_two: &[],
    actions: &[
        any("steer_left", "Steer left", 1, &["Left", "PadLeft"]),
        any("steer_right", "Steer right", 2, &["Right", "PadRight"]),
        any("gas", "Gas", 3, &["Up", "PadRT"]),
        any("brake", "Brake", 4, &["Down", "PadLT"]),
        any("shift_up", "Shift up", 0x200, &["E", "PadRB"]),
        any("shift_down", "Shift down", 0x201, &["Q", "PadLB"]),
        any("gear1", "Gear 1", 0x211, &["F1"]),
        any("gear2", "Gear 2", 0x212, &["F2"]),
        any("gear3", "Gear 3", 0x213, &["F3"]),
        any("gear4", "Gear 4", 0x214, &["F4"]),
        any("gear5", "Gear 5", 0x215, &["F5"]),
        any("gear6", "Gear 6", 0x216, &["F6"]),
        any("neutral", "Neutral", 0x210, &["N"]),
        any("enter", "Enter (view change)", 0x100, &["Enter", "PadA", "PadStart"]),
        any("perspective", "Perspective", 0x101, &["V", "PadY"]),
        any("interrupt", "Interruption", 0x102, &["B", "PadX"]),
        any("coin", "Coin", 0x103, &["5", "PadBack"]),
        any("test", "Test (operator menu)", 0x104, &["T"]),
        any("service", "Service", 0x105, &["9"]),
        any("menu_up", "Operator menu up", 0x106, &["PageUp", "PadUp"]),
        any("menu_down", "Operator menu down", 0x107, &["PageDown", "PadDown"]),
    ],
};
