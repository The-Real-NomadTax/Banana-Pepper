//! A session's controller settings as the launcher passes them to game
//! adapters (`ARCADE_PLAYERS`, `ARCADE_INPUTS`, `ARCADE_AIM_MODES`,
//! `ARCADE_MOVE[_P2]`, `ARCADE_PAD[_P2]`), plus press/release edges of
//! pad-bound targets. Platform-neutral: the same adapter code reads these on
//! Linux (evdev) and Windows (XInput, DirectInput, Raw Input).
use crate::pad::{self, Pad};
use std::{sync::OnceLock, time::Instant};

/// Who plays with which device (settings.cfg `players`, `p1_input`,
/// `p2_input`, via `ARCADE_PLAYERS` and `ARCADE_INPUTS`).
pub fn inputs() -> pad::Inputs {
    static INPUTS: OnceLock<pad::Inputs> = OnceLock::new();
    *INPUTS.get_or_init(|| {
        let var = |name| std::env::var(name).ok();
        let (players, inputs) = (var("ARCADE_PLAYERS"), var("ARCADE_INPUTS"));
        pad::Inputs::parse(
            players.as_deref().map(str::as_bytes),
            inputs.as_deref().map(str::as_bytes),
        )
    })
}

/// Stick-aim mode for this player's controller. Pointer devices stay direct.
pub fn aim_mode(player: usize) -> pad::AimMode {
    static MODES: OnceLock<[pad::AimMode; 2]> = OnceLock::new();
    let modes = MODES.get_or_init(|| {
        let text = std::env::var("ARCADE_AIM_MODES").ok();
        [0, 1].map(|p| pad::AimMode::for_player(text.as_deref().map(str::as_bytes), p))
    });
    match inputs().methods[player.min(1)] {
        pad::Method::XInput | pad::Method::DirectInput => modes[player.min(1)],
        _ => pad::AimMode::Cursor,
    }
}

/// A player's movement axes (`ARCADE_MOVE`, `ARCADE_MOVE_P2`), or `default`.
pub fn movement(player: usize, default: pad::Movement) -> pad::Movement {
    let name = if player == 0 {
        "ARCADE_MOVE"
    } else {
        "ARCADE_MOVE_P2"
    };
    let text = std::env::var(name).ok();
    pad::Movement::parse(text.as_deref().map(str::as_bytes), default)
}

/// `ARCADE_PAD` bindings (`target:mask,...`), or `defaults` when unset.
pub fn bindings(defaults: &str) -> Vec<(u32, u32)> {
    parse(&std::env::var("ARCADE_PAD").unwrap_or_else(|_| defaults.to_owned()))
}

/// Controller 2's bindings (`ARCADE_PAD_P2`), or `defaults` when unset.
pub fn bindings_p2(defaults: &str) -> Vec<(u32, u32)> {
    parse(&std::env::var("ARCADE_PAD_P2").unwrap_or_else(|_| defaults.to_owned()))
}

fn parse(text: &str) -> Vec<(u32, u32)> {
    let mut out = vec![(0, 0); 128];
    let n = pad::parse_bindings(text.as_bytes(), &mut out);
    out.truncate(n);
    out
}

/// Reports press/release edges of pad-bound targets, so controller input
/// never overrides a key that is held at the same time.
#[derive(Default)]
pub struct Edges {
    held: Vec<(u32, bool)>,
}

impl Edges {
    /// Each player's controller with its own bindings. A target shared by
    /// both (menus, test) is held while either player holds it.
    pub fn update_players(
        &mut self,
        players: &[(Pad, &[(u32, u32)])],
        mut changed: impl FnMut(u32, bool),
    ) {
        for (_, bindings) in players {
            for &(target, _) in *bindings {
                if !self.held.iter().any(|h| h.0 == target) {
                    self.held.push((target, false));
                }
            }
        }
        for (target, was) in &mut self.held {
            let now = players
                .iter()
                .any(|(state, bindings)| pad::target_held(state, bindings, *target));
            if now != *was {
                *was = now;
                changed(*target, now);
            }
        }
    }
}

/// The per-frame controller work every game adapter shares: who plays with
/// what, each player's bindings and aim axes, press/release edges of bound
/// targets, frame timing, the Back + Start exit chord and pointer-or-stick
/// aiming. Adapters read the states from their platform's devices and turn
/// the results into the game's own input (switches, events, keys).
pub struct Players {
    pub inputs: pad::Inputs,
    /// Each player's controller buttons; player two's are empty in
    /// one-player sessions.
    pub bindings: [Vec<(u32, u32)>; pad::MAX_PLAYERS],
    /// Each player's aim axes (controls.cfg `move_x` / `move_y`).
    pub moves: [pad::Movement; pad::MAX_PLAYERS],
    edges: Edges,
    last: Instant,
}

impl Players {
    /// The session's players, with these default bindings for controllers
    /// that controls.cfg leaves unset.
    pub fn new(default_pad: &str, default_pad_p2: &str) -> Self {
        let inputs = inputs();
        Self {
            bindings: [
                bindings(default_pad),
                if inputs.players > 1 {
                    bindings_p2(default_pad_p2)
                } else {
                    Vec::new()
                },
            ],
            inputs,
            moves: [0, 1].map(|p| movement(p, pad::Movement::STICKS)),
            edges: Edges::default(),
            last: Instant::now(),
        }
    }

    /// Milliseconds since the previous frame.
    pub fn tick(&mut self) -> u32 {
        let elapsed = self.last.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
        self.last = Instant::now();
        elapsed
    }

    /// Back + Start held together on any player's controller: leave the game.
    pub fn quit(states: &[Pad]) -> bool {
        states
            .iter()
            .any(|s| s.held(pad::BACK) && s.held(pad::START))
    }

    /// Press/release edges of every player's bound targets.
    pub fn edges(&mut self, states: &[Pad; pad::MAX_PLAYERS], changed: impl FnMut(u32, bool)) {
        let players: Vec<(Pad, &[(u32, u32)])> = (0..self.inputs.players)
            .map(|p| (states[p], self.bindings[p].as_slice()))
            .collect();
        self.edges.update_players(&players, changed);
    }

    /// Move `player`'s aim `cursor`: a mouse or light gun of the player's own
    /// points directly; otherwise the stick steps it (aim mode, `speed`
    /// pixels per second at full deflection). True when it moved.
    #[allow(clippy::too_many_arguments)]
    pub fn aim(
        &self,
        player: usize,
        state: &Pad,
        cursor: &mut pad::Cursor,
        elapsed: u32,
        speed: i32,
        width: i32,
        height: i32,
    ) -> bool {
        let player = player.min(pad::MAX_PLAYERS - 1);
        match state.pointer_moved(&self.moves[player], width, height) {
            Some((x, y)) if (x, y) != (cursor.x, cursor.y) => {
                *cursor = pad::Cursor::new(x, y);
                true
            }
            Some(_) => false,
            None => cursor.step_mode(
                aim_mode(player),
                state.movement(&self.moves[player]),
                elapsed,
                speed,
                width,
                height,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_and_start_together_quit() {
        let both = Pad {
            buttons: pad::BACK | pad::START,
            ..Pad::default()
        };
        let back = Pad {
            buttons: pad::BACK,
            ..Pad::default()
        };
        assert!(Players::quit(&[Pad::default(), both]));
        assert!(!Players::quit(&[back, Pad::default()]));
    }

    #[test]
    fn edges_report_changes_once() {
        let bindings = [(5, pad::START), (5, pad::A), (7, pad::BACK)];
        let mut edges = Edges::default();
        let mut log = Vec::new();
        let pressed = Pad {
            buttons: pad::A,
            ..Pad::default()
        };
        edges.update_players(&[(pressed, &bindings)], |t, d| log.push((t, d)));
        edges.update_players(&[(pressed, &bindings)], |t, d| log.push((t, d)));
        edges.update_players(&[(Pad::default(), &bindings)], |t, d| log.push((t, d)));
        assert_eq!(log, [(5, true), (5, false)]);
    }

    #[test]
    fn each_player_uses_its_own_bindings() {
        let p1 = [(5, pad::START), (11, pad::BACK)];
        let p2 = [(6, pad::START), (11, pad::BACK)];
        let start = Pad {
            buttons: pad::START,
            ..Pad::default()
        };
        let back = Pad {
            buttons: pad::BACK,
            ..Pad::default()
        };
        let mut edges = Edges::default();
        let mut log = Vec::new();
        edges.update_players(&[(Pad::default(), &p1), (start, &p2)], |t, d| {
            log.push((t, d))
        });
        assert_eq!(log, [(6, true)]);
        log.clear();
        // A shared target stays held while either player holds it.
        edges.update_players(&[(back, &p1), (back, &p2)], |t, d| log.push((t, d)));
        edges.update_players(&[(back, &p1), (Pad::default(), &p2)], |t, d| {
            log.push((t, d))
        });
        assert_eq!(log, [(11, true), (6, false)]);
    }
}
