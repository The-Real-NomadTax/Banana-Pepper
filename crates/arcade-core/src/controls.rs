//! User-customizable desktop key bindings, stored in `controls.cfg`.
//!
//! The file is plain text with one section per game:
//!
//! ```text
//! [aliens]
//! test = T
//! start1 = 1
//!
//! [jurassic]
//! menu_select = Enter, Keypad5
//! ```
//!
//! Missing games/actions keep their defaults. Escape is reserved for exiting.
use std::{collections::BTreeMap, fs, io, path::Path};

/// How a game receives key codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySpace {
    /// SDL 1.2 key symbols (Aliens, Jurassic Park).
    Sdl,
    /// GLUT ASCII keyboard callback (Big Buck Hunter); no arrow/function keys.
    Glut,
    /// DirectInput DIK scan codes (Super Cars, read by the Wine hook DLL).
    DirectInput,
}

#[derive(Debug)]
pub struct Action {
    pub id: &'static str,
    pub label: &'static str,
    /// Game-specific destination: Aliens cabinet switch source, Jurassic input
    /// target or Big Buck switch index.
    pub target: u32,
    pub default: &'static [&'static str],
    /// Exactly one keyboard key is required (Aliens patches one operand per
    /// action). Controller buttons do not count toward this.
    pub single: bool,
    /// Only controller buttons may be bound (e.g. Big Buck's aimed shot).
    pub pad_only: bool,
}

#[derive(Debug)]
pub struct Game {
    pub id: &'static str,
    pub title: &'static str,
    pub keys: KeySpace,
    /// MouseLeft / MouseRight may be bound.
    pub mouse_buttons: bool,
    pub actions: &'static [Action],
    pub note: &'static str,
}

const fn one(
    id: &'static str,
    label: &'static str,
    target: u32,
    key: &'static [&'static str],
) -> Action {
    Action {
        id,
        label,
        target,
        default: key,
        single: true,
        pad_only: false,
    }
}
const fn any(
    id: &'static str,
    label: &'static str,
    target: u32,
    keys: &'static [&'static str],
) -> Action {
    Action {
        id,
        label,
        target,
        default: keys,
        single: false,
        pad_only: false,
    }
}
const fn pad(
    id: &'static str,
    label: &'static str,
    target: u32,
    buttons: &'static [&'static str],
) -> Action {
    Action {
        id,
        label,
        target,
        default: buttons,
        single: false,
        pad_only: true,
    }
}

pub const GAMES: &[Game] = &[
    Game {id:"ford",title:"Ford Racing Full Blown",keys:KeySpace::DirectInput,mouse_buttons:false,note:"Keyboard steering ramps and recentres. Controllers (XInput): left stick steers, triggers bound to gas/brake are analog. The cabinet adapter runs on Windows and Wine; Escape exits.",actions:&[
        any("steer_left","Steer left",1,&["Left","PadLeft"]),any("steer_right","Steer right",2,&["Right","PadRight"]),any("gas","Gas / menu confirm",3,&["Up","PadRT"]),any("brake","Brake",4,&["Down","PadLT"]),
        any("start1","Start",5,&["Enter","1","PadStart","PadA"]),any("view","Change view",6,&["V","PadY"]),any("gear_up","Shift up",7,&["Space","PadRB"]),any("gear_down","Shift down",8,&["X","PadLB"]),any("coin1","Coin",9,&["5","PadBack"]),
    ]},
    Game {
        id: "aliens",
        title: "Aliens Armageddon",
        keys: KeySpace::Sdl,
        mouse_buttons: false,
        note: "Mouse aim and fire use the game's own desktop mouse controls. One key per action (plus any controller buttons). Controllers: either stick moves the pointer, RT = left click, LT = right click.",
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
    },
    Game {
        id: "jurassic",
        title: "Jurassic Park 2015",
        keys: KeySpace::Sdl,
        mouse_buttons: true,
        note: "Mouse movement always aims for player one. Controllers: either stick moves the aim.",
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
    },
    Game {
        id: "big_buck",
        title: "Big Buck Hunter Pro",
        keys: KeySpace::Glut,
        mouse_buttons: false,
        note: "Left mouse fires and right mouse pumps. Letters, digits, Space, Enter, Tab, Backspace and punctuation only. Controllers: either stick moves the crosshair.",
        actions: &[
            any("start1", "Player 1 Start", 5, &["1", "Enter", "PadStart"]),
            any("start2", "Player 2 Start", 6, &["2"]),
            any("coin1", "Coin 1", 7, &["5", "PadBack"]),
            any("coin2", "Coin 2", 8, &["6"]),
            any("test", "Test (operator menu)", 11, &["T"]),
            any("service", "Service", 10, &["9"]),
            any("trigger", "Trigger switch (keyboard)", 1, &["Space"]),
            pad("fire", "Fire at the crosshair (controller)", 0x100, &["PadRT", "PadA"]),
            pad("pump", "Pump (controller)", 0x101, &["PadLT", "PadB"]),
        ],
    },
    Game {
        id: "super_cars",
        title: "Fast and Furious Super Cars",
        keys: KeySpace::DirectInput,
        mouse_buttons: false,
        note: "Steering ramps while held and recentres on release; gas and brake keys are on/off. Controllers (XInput): left stick steers, triggers bound to gas/brake are analog. Targets are the cabinet I/O board's input codes.",
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
    },
];

pub fn game(id: &str) -> Option<&'static Game> {
    GAMES.iter().find(|g| g.id == id)
}

/// SDL 1.2 key symbols by canonical name.
const SDL_KEYS: &[(&str, u32)] = &[
    ("Backspace", 8),
    ("Tab", 9),
    ("Enter", 13),
    ("Pause", 19),
    ("Space", 32),
    ("Quote", 39),
    ("Comma", 44),
    ("Minus", 45),
    ("Period", 46),
    ("Slash", 47),
    ("Semicolon", 59),
    ("Equals", 61),
    ("LeftBracket", 91),
    ("Backslash", 92),
    ("RightBracket", 93),
    ("Backquote", 96),
    ("Delete", 127),
    ("Keypad0", 256),
    ("Keypad1", 257),
    ("Keypad2", 258),
    ("Keypad3", 259),
    ("Keypad4", 260),
    ("Keypad5", 261),
    ("Keypad6", 262),
    ("Keypad7", 263),
    ("Keypad8", 264),
    ("Keypad9", 265),
    ("KeypadPeriod", 266),
    ("KeypadDivide", 267),
    ("KeypadMultiply", 268),
    ("KeypadMinus", 269),
    ("KeypadPlus", 270),
    ("KeypadEnter", 271),
    ("Up", 273),
    ("Down", 274),
    ("Right", 275),
    ("Left", 276),
    ("Insert", 277),
    ("Home", 278),
    ("End", 279),
    ("PageUp", 280),
    ("PageDown", 281),
    ("F1", 282),
    ("F2", 283),
    ("F3", 284),
    ("F4", 285),
    ("F5", 286),
    ("F6", 287),
    ("F7", 288),
    ("F8", 289),
    ("F9", 290),
    ("F10", 291),
    ("F11", 292),
    ("F12", 293),
    ("RightShift", 303),
    ("LeftShift", 304),
    ("RightCtrl", 305),
    ("LeftCtrl", 306),
    ("RightAlt", 307),
    ("LeftAlt", 308),
    // Jurassic raw IO mouse-button sources (same enum as its SDL keys).
    ("MouseLeft", 326),
    ("MouseRight", 327),
];

const ALIASES: &[(&str, &str)] = &[
    ("return", "Enter"),
    ("esc", "Escape"),
    ("arrowup", "Up"),
    ("arrowdown", "Down"),
    ("arrowleft", "Left"),
    ("arrowright", "Right"),
    ("pgup", "PageUp"),
    ("pgdn", "PageDown"),
    ("pagedown", "PageDown"),
    ("del", "Delete"),
    ("ins", "Insert"),
    ("apostrophe", "Quote"),
    ("openbracket", "LeftBracket"),
    ("closebracket", "RightBracket"),
    ("backtick", "Backquote"),
    ("grave", "Backquote"),
    ("equal", "Equals"),
    ("dash", "Minus"),
    ("spacebar", "Space"),
    ("shift", "LeftShift"),
    ("ctrl", "LeftCtrl"),
    ("control", "LeftCtrl"),
    ("alt", "LeftAlt"),
    ("mouse1", "MouseLeft"),
    ("mouse2", "MouseRight"),
    ("leftmouse", "MouseLeft"),
    ("rightmouse", "MouseRight"),
    ("kpenter", "KeypadEnter"),
    ("kpplus", "KeypadPlus"),
    ("kpminus", "KeypadMinus"),
];

/// Controller buttons by canonical name, with their XInput `wButtons` mask
/// (Xbox layout). The triggers have no XInput button bit; they use the two
/// bits above it and count as pressed past a quarter of their travel.
/// Every backend reads controllers into this same mask: XInput on Windows
/// and Wine, evdev on Linux.
pub const PAD_BUTTONS: &[(&str, u32, &str)] = &[
    ("PadA", 0x1000, "A"),
    ("PadB", 0x2000, "B"),
    ("PadX", 0x4000, "X"),
    ("PadY", 0x8000, "Y"),
    ("PadLB", 0x0100, "Left bumper"),
    ("PadRB", 0x0200, "Right bumper"),
    ("PadLT", 0x1_0000, "Left trigger"),
    ("PadRT", 0x2_0000, "Right trigger"),
    ("PadBack", 0x0020, "Back / View"),
    ("PadStart", 0x0010, "Start / Menu"),
    ("PadLS", 0x0040, "Left stick click"),
    ("PadRS", 0x0080, "Right stick click"),
    ("PadUp", 0x0001, "D-pad up"),
    ("PadDown", 0x0002, "D-pad down"),
    ("PadLeft", 0x0004, "D-pad left"),
    ("PadRight", 0x0008, "D-pad right"),
];
pub const PAD_LT: u32 = 0x1_0000;
pub const PAD_RT: u32 = 0x2_0000;

const PAD_ALIASES: &[(&str, &str)] = &[
    ("padview", "PadBack"),
    ("padselect", "PadBack"),
    ("padmenu", "PadStart"),
    ("padl1", "PadLB"),
    ("padr1", "PadRB"),
    ("padl2", "PadLT"),
    ("padr2", "PadRT"),
    ("padl3", "PadLS"),
    ("padr3", "PadRS"),
    ("paddpadup", "PadUp"),
    ("paddpaddown", "PadDown"),
    ("paddpadleft", "PadLeft"),
    ("paddpadright", "PadRight"),
];

/// XInput-style mask for a canonical controller button name.
pub fn pad_mask(canonical: &str) -> Option<u32> {
    PAD_BUTTONS
        .iter()
        .find(|(n, _, _)| *n == canonical)
        .map(|(_, m, _)| *m)
}

pub fn is_pad(canonical: &str) -> bool {
    pad_mask(canonical).is_some()
}

/// Canonical key name for a user/egui spelling, e.g. "Num1" → "1", "ArrowUp" → "Up".
pub fn normalize(name: &str) -> Option<String> {
    let name = name.trim();
    let lower = name.to_ascii_lowercase();
    if let Some((n, _, _)) = PAD_BUTTONS.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name)) {
        return Some((*n).to_owned());
    }
    if let Some(&(_, canonical)) = PAD_ALIASES.iter().find(|(alias, _)| *alias == lower) {
        return Some(canonical.to_owned());
    }
    if name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric() {
        return Some(name.to_ascii_uppercase());
    }
    if let Some(digit) = lower
        .strip_prefix("num")
        .or_else(|| lower.strip_prefix("digit"))
        && digit.len() == 1
        && digit.as_bytes()[0].is_ascii_digit()
    {
        return Some(digit.to_owned());
    }
    if let Some(rest) = lower
        .strip_prefix("kp")
        .or_else(|| lower.strip_prefix("numpad"))
        && rest.len() == 1
        && rest.as_bytes()[0].is_ascii_digit()
    {
        return Some(format!("Keypad{rest}"));
    }
    if let Some(&(_, canonical)) = ALIASES.iter().find(|(alias, _)| *alias == lower) {
        return Some(canonical.to_owned());
    }
    if lower == "escape" {
        return Some("Escape".into());
    }
    let punctuation = match name {
        "-" => Some("Minus"),
        "=" => Some("Equals"),
        "," => Some("Comma"),
        "." => Some("Period"),
        "/" => Some("Slash"),
        ";" => Some("Semicolon"),
        "'" => Some("Quote"),
        "[" => Some("LeftBracket"),
        "]" => Some("RightBracket"),
        "\\" => Some("Backslash"),
        "`" => Some("Backquote"),
        _ => None,
    };
    if let Some(p) = punctuation {
        return Some(p.into());
    }
    SDL_KEYS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(n, _)| (*n).to_owned())
}

/// DirectInput DIK scan codes by canonical name (letters and digits included).
const DIK_KEYS: &[(&str, u32)] = &[
    ("1", 0x02),
    ("2", 0x03),
    ("3", 0x04),
    ("4", 0x05),
    ("5", 0x06),
    ("6", 0x07),
    ("7", 0x08),
    ("8", 0x09),
    ("9", 0x0a),
    ("0", 0x0b),
    ("Minus", 0x0c),
    ("Equals", 0x0d),
    ("Backspace", 0x0e),
    ("Tab", 0x0f),
    ("Q", 0x10),
    ("W", 0x11),
    ("E", 0x12),
    ("R", 0x13),
    ("T", 0x14),
    ("Y", 0x15),
    ("U", 0x16),
    ("I", 0x17),
    ("O", 0x18),
    ("P", 0x19),
    ("LeftBracket", 0x1a),
    ("RightBracket", 0x1b),
    ("Enter", 0x1c),
    ("LeftCtrl", 0x1d),
    ("A", 0x1e),
    ("S", 0x1f),
    ("D", 0x20),
    ("F", 0x21),
    ("G", 0x22),
    ("H", 0x23),
    ("J", 0x24),
    ("K", 0x25),
    ("L", 0x26),
    ("Semicolon", 0x27),
    ("Quote", 0x28),
    ("Backquote", 0x29),
    ("LeftShift", 0x2a),
    ("Backslash", 0x2b),
    ("Z", 0x2c),
    ("X", 0x2d),
    ("C", 0x2e),
    ("V", 0x2f),
    ("B", 0x30),
    ("N", 0x31),
    ("M", 0x32),
    ("Comma", 0x33),
    ("Period", 0x34),
    ("Slash", 0x35),
    ("RightShift", 0x36),
    ("KeypadMultiply", 0x37),
    ("LeftAlt", 0x38),
    ("Space", 0x39),
    ("F1", 0x3b),
    ("F2", 0x3c),
    ("F3", 0x3d),
    ("F4", 0x3e),
    ("F5", 0x3f),
    ("F6", 0x40),
    ("F7", 0x41),
    ("F8", 0x42),
    ("F9", 0x43),
    ("F10", 0x44),
    ("Keypad7", 0x47),
    ("Keypad8", 0x48),
    ("Keypad9", 0x49),
    ("KeypadMinus", 0x4a),
    ("Keypad4", 0x4b),
    ("Keypad5", 0x4c),
    ("Keypad6", 0x4d),
    ("KeypadPlus", 0x4e),
    ("Keypad1", 0x4f),
    ("Keypad2", 0x50),
    ("Keypad3", 0x51),
    ("Keypad0", 0x52),
    ("KeypadPeriod", 0x53),
    ("F11", 0x57),
    ("F12", 0x58),
    ("KeypadEnter", 0x9c),
    ("RightCtrl", 0x9d),
    ("KeypadDivide", 0xb5),
    ("RightAlt", 0xb8),
    ("Pause", 0xc5),
    ("Home", 0xc7),
    ("Up", 0xc8),
    ("PageUp", 0xc9),
    ("Left", 0xcb),
    ("Right", 0xcd),
    ("End", 0xcf),
    ("Down", 0xd0),
    ("PageDown", 0xd1),
    ("Insert", 0xd2),
    ("Delete", 0xd3),
];

/// Numeric code a game receives for a canonical key name.
pub fn code(keys: KeySpace, canonical: &str) -> Option<u32> {
    let single = (canonical.len() == 1).then(|| canonical.as_bytes()[0]);
    match keys {
        KeySpace::DirectInput => DIK_KEYS
            .iter()
            .find(|(n, _)| *n == canonical)
            .map(|(_, c)| *c),
        KeySpace::Sdl => match single {
            Some(c) if c.is_ascii_alphanumeric() => Some(u32::from(c.to_ascii_lowercase())),
            _ => SDL_KEYS
                .iter()
                .find(|(n, _)| *n == canonical)
                .map(|(_, c)| *c),
        },
        KeySpace::Glut => match single {
            Some(c) if c.is_ascii_alphanumeric() => Some(u32::from(c.to_ascii_lowercase())),
            _ => match SDL_KEYS.iter().find(|(n, _)| *n == canonical) {
                Some((_, c)) if *c < 128 => Some(*c),
                _ => None,
            },
        },
    }
}

fn check_key(game: &Game, action: &Action, name: &str) -> Result<String, String> {
    let canonical = normalize(name).ok_or_else(|| format!("unknown key \"{name}\""))?;
    if is_pad(&canonical) {
        return Ok(canonical);
    }
    if action.pad_only {
        return Err(format!("\"{}\" takes controller buttons only", action.label));
    }
    if canonical == "Escape" {
        return Err("Escape is reserved for exiting the game".into());
    }
    if canonical.starts_with("Mouse") && !game.mouse_buttons {
        return Err(format!("{} cannot bind mouse buttons here", game.title));
    }
    if code(game.keys, &canonical).is_none() {
        return Err(format!("{} cannot use the {canonical} key", game.title));
    }
    Ok(canonical)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings {
    keys: BTreeMap<(String, String), Vec<String>>,
}

impl Default for Bindings {
    fn default() -> Self {
        let mut keys = BTreeMap::new();
        for game in GAMES {
            for action in game.actions {
                keys.insert(
                    (game.id.to_owned(), action.id.to_owned()),
                    action.default.iter().map(|k| (*k).to_owned()).collect(),
                );
            }
        }
        Self { keys }
    }
}

impl Bindings {
    pub fn keys(&self, game: &str, action: &str) -> &[String] {
        self.keys
            .get(&(game.to_owned(), action.to_owned()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Replace an action's keys after normalizing them. Validation of the whole
    /// set (duplicates, single-key actions) happens in [`Bindings::validate`].
    pub fn set(&mut self, game: &str, action: &str, keys: &[&str]) -> Result<(), String> {
        let g = self::game(game).ok_or_else(|| format!("unknown game \"{game}\""))?;
        let a = g
            .actions
            .iter()
            .find(|a| a.id == action)
            .ok_or_else(|| format!("unknown {} action \"{action}\"", g.title))?;
        let mut canonical = Vec::new();
        for key in keys {
            let key = check_key(g, a, key)?;
            if !canonical.contains(&key) {
                canonical.push(key);
            }
        }
        // Keyboard/mouse first, then controller buttons (stable within each).
        canonical.sort_by_key(|k| is_pad(k));
        self.keys
            .insert((game.to_owned(), action.to_owned()), canonical);
        Ok(())
    }

    pub fn reset_game(&mut self, game: &str) {
        let defaults = Self::default();
        for ((g, a), keys) in defaults.keys {
            if g == game {
                self.keys.insert((g, a), keys);
            }
        }
    }

    /// Actions in `game` other than `except` that already use `key`.
    pub fn conflicts(&self, game: &str, except: &str, key: &str) -> Vec<&'static str> {
        let Some(g) = self::game(game) else {
            return vec![];
        };
        g.actions
            .iter()
            .filter(|a| a.id != except && self.keys(game, a.id).iter().any(|k| k == key))
            .map(|a| a.id)
            .collect()
    }

    /// Bind `key` to an action (replacing its keys, or adding with `append`).
    /// A key already used by another action moves; an action left without keys
    /// takes this action's previous keys (a swap). Nothing changes on error.
    /// Returns a short description of what changed.
    pub fn rebind(
        &mut self,
        game: &str,
        action: &str,
        key: &str,
        append: bool,
    ) -> Result<String, String> {
        let g = self::game(game).ok_or_else(|| format!("unknown game \"{game}\""))?;
        let a = g
            .actions
            .iter()
            .find(|a| a.id == action)
            .ok_or_else(|| format!("unknown {} action \"{action}\"", g.title))?;
        let key = check_key(g, a, key)?;
        let mut next = self.clone();
        let old = next.keys(game, action).to_vec();
        // Replacing a key keeps the action's controller buttons.
        let mut keys = if append {
            old.clone()
        } else {
            old.iter().filter(|k| is_pad(k)).cloned().collect()
        };
        if !keys.contains(&key) {
            keys.push(key.clone());
        }
        let mut notes = Vec::new();
        for other in next.conflicts(game, action, &key) {
            let mut remaining: Vec<String> = next
                .keys(game, other)
                .iter()
                .filter(|k| k.as_str() != key)
                .cloned()
                .collect();
            // Left without a keyboard key: take this action's previous keys (a swap).
            if !remaining.iter().any(|k| !is_pad(k)) {
                remaining.extend(
                    old.iter()
                        .filter(|k| !keys.contains(k) && !is_pad(k))
                        .cloned(),
                );
            }
            let refs: Vec<&str> = remaining.iter().map(String::as_str).collect();
            next.set(game, other, &refs)?;
            notes.push(format!(
                "{other} is now {}",
                if remaining.is_empty() {
                    "unbound".to_owned()
                } else {
                    remaining.join(", ")
                }
            ));
        }
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        next.set(game, action, &refs)?;
        next.validate()?;
        *self = next;
        let mut message = format!("{} {action} = {}", g.title, keys.join(", "));
        if !notes.is_empty() {
            message += &format!(" ({})", notes.join("; "));
        }
        Ok(message)
    }

    /// Remove all keys from an action that may be unbound.
    pub fn clear(&mut self, game: &str, action: &str) -> Result<(), String> {
        let mut next = self.clone();
        next.set(game, action, &[])?;
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        for game in GAMES {
            let mut seen: BTreeMap<String, &str> = BTreeMap::new();
            for action in game.actions {
                let keys = self.keys(game.id, action.id);
                if action.single && keys.iter().filter(|k| !is_pad(k)).count() != 1 {
                    return Err(format!(
                        "{}: \"{}\" needs exactly one key",
                        game.title, action.label
                    ));
                }
                for key in keys {
                    check_key(game, action, key).map_err(|e| format!("{}: {e}", game.title))?;
                    if let Some(other) = seen.insert(key.clone(), action.id) {
                        return Err(format!(
                            "{}: {key} is bound to both \"{other}\" and \"{}\"",
                            game.title, action.id
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// (target, code) keyboard/mouse pairs for one game, in action order.
    /// Controller buttons are reported separately by [`Bindings::pad_codes`].
    pub fn codes(&self, game: &str) -> Result<Vec<(u32, u32)>, String> {
        self.validate()?;
        let g = self::game(game).ok_or_else(|| format!("unknown game \"{game}\""))?;
        let mut pairs = Vec::new();
        for action in g.actions {
            for key in self.keys(game, action.id).iter().filter(|k| !is_pad(k)) {
                pairs.push((action.target, code(g.keys, key).expect("validated key")));
            }
        }
        Ok(pairs)
    }

    /// (target, controller button mask) pairs for one game, in action order.
    pub fn pad_codes(&self, game: &str) -> Result<Vec<(u32, u32)>, String> {
        self.validate()?;
        let g = self::game(game).ok_or_else(|| format!("unknown game \"{game}\""))?;
        let mut pairs = Vec::new();
        for action in g.actions {
            for key in self.keys(game, action.id) {
                if let Some(mask) = pad_mask(key) {
                    pairs.push((action.target, mask));
                }
            }
        }
        Ok(pairs)
    }

    /// Compact `target:mask,...` controller form passed to the game adapters
    /// (`ARCADE_PAD`). Analog behaviour (sticks, analog triggers) is fixed
    /// per game and described in each game's note.
    pub fn pad_env_value(&self, game: &str) -> Result<String, String> {
        Ok(self
            .pad_codes(game)?
            .iter()
            .map(|(t, m)| format!("{t}:{m}"))
            .collect::<Vec<_>>()
            .join(","))
    }

    /// `keycode:mask,...` for games whose adapter presses the action's own
    /// keyboard key when a controller button is held (Aliens, whose image is
    /// patched to one key per action). Uses each action's first key.
    pub fn pad_key_env_value(&self, game: &str) -> Result<String, String> {
        self.validate()?;
        let g = self::game(game).ok_or_else(|| format!("unknown game \"{game}\""))?;
        let mut pairs = Vec::new();
        for action in g.actions {
            let keys = self.keys(game, action.id);
            let Some(key) = keys.iter().find(|k| !is_pad(k)) else {
                continue;
            };
            let Some(code) = code(g.keys, key) else {
                continue;
            };
            for mask in keys.iter().filter_map(|k| pad_mask(k)) {
                pairs.push(format!("{code}:{mask}"));
            }
        }
        Ok(pairs.join(","))
    }

    /// Add or remove one controller button on an action. Adding moves the
    /// button off any other action in the same game.
    pub fn toggle_pad(&mut self, game: &str, action: &str, button: &str) -> Result<String, String> {
        let g = self::game(game).ok_or_else(|| format!("unknown game \"{game}\""))?;
        let button = normalize(button)
            .filter(|b| is_pad(b))
            .ok_or_else(|| format!("unknown controller button \"{button}\""))?;
        let mut next = self.clone();
        let mut keys = next.keys(game, action).to_vec();
        let message;
        if let Some(i) = keys.iter().position(|k| *k == button) {
            keys.remove(i);
            message = format!("{} {action}: removed {button}", g.title);
        } else {
            for other in next.conflicts(game, action, &button) {
                let rest: Vec<String> = next
                    .keys(game, other)
                    .iter()
                    .filter(|k| **k != button)
                    .cloned()
                    .collect();
                let refs: Vec<&str> = rest.iter().map(String::as_str).collect();
                next.set(game, other, &refs)?;
            }
            keys.push(button.clone());
            message = format!("{} {action}: added {button}", g.title);
        }
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        next.set(game, action, &refs)?;
        next.validate()?;
        *self = next;
        Ok(message)
    }

    /// Compact `target:code,...` form passed to the guest shim.
    pub fn env_value(&self, game: &str) -> Result<String, String> {
        Ok(self
            .codes(game)?
            .iter()
            .map(|(t, c)| format!("{t}:{c}"))
            .collect::<Vec<_>>()
            .join(","))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut bindings = Self::default();
        let mut section: Option<&'static Game> = None;
        for (number, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            let at = |e: String| format!("controls.cfg line {}: {e}", number + 1);
            if line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section =
                    Some(game(name.trim()).ok_or_else(|| at(format!("unknown game [{name}]")))?);
                continue;
            }
            let g = section.ok_or_else(|| at("binding before any [game] section".into()))?;
            let (action, keys) = line
                .split_once('=')
                .ok_or_else(|| at("expected action = key".into()))?;
            let keys: Vec<&str> = keys
                .split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .collect();
            bindings.set(g.id, action.trim(), &keys).map_err(at)?;
        }
        bindings.validate()?;
        Ok(bindings)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from(
            "# Banana pepper desktop controls. Edit here or in the workbench's Controls panel.\n\
             # Separate multiple keys with commas. Escape always exits a game.\n",
        );
        for game in GAMES {
            out += &format!("\n[{}]  # {}. {}\n", game.id, game.title, game.note);
            for action in game.actions {
                out += &format!(
                    "{} = {}  # {}\n",
                    action.id,
                    self.keys(game.id, action.id).join(", "),
                    action.label
                );
            }
        }
        out
    }

    /// Missing file → defaults.
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Cannot read {}: {e}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let temp = path.with_extension("cfg.saving");
        fs::write(&temp, self.to_text())
            .and_then(|_| fs::rename(&temp, path))
            .map_err(|e| format!("Cannot save {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_match_previous_fixed_keys() {
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
    fn round_trip_and_overrides() {
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
}
