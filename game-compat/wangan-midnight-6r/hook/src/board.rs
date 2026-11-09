//! A JVS I/O board, as the cabinet's driver sees it on its serial port.
//!
//! The game writes request packets and reads replies through the same
//! handle. Framing follows the JVS specification: `E0` sync, destination,
//! length (payload + checksum), payload, checksum (byte sum of everything
//! after the sync). `E0` and `D0` inside a packet are sent as `D0, byte - 1`.
//! Every command in a request gets its own report byte in the reply.
//!
//! No `std` use, so the protocol is unit-tested on the host.

/// Identification string (command 0x10).
const IDENTITY: &[u8] = b"Banana Pepper;JVS I/O;Ver1.00;Maximum Tune 6R\0";
/// Switch inputs: players and switches per player.
const PLAYERS: u8 = 2;
const SWITCHES: u8 = 16;
/// Analog channels and their resolution in bits.
const CHANNELS: u8 = 8;
const ANALOG_BITS: u8 = 16;
const COIN_SLOTS: usize = 2;

const SYNC: u8 = 0xe0;
const MARK: u8 = 0xd0;
const BROADCAST: u8 = 0xff;
const REPORT_OK: u8 = 1;
const REPORT_PARAMETER: u8 = 2;
const STATUS_OK: u8 = 1;
const STATUS_UNKNOWN: u8 = 2;
const STATUS_CHECKSUM: u8 = 3;

/// What the board reads from the desktop when the game polls it.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Inputs {
    /// System byte (bit 7 = test) and two bytes per player, in JVS order.
    pub switches: [u8; 1 + 2 * PLAYERS as usize],
    /// Unsigned 16-bit channels (0x8000 = centre).
    pub analog: [u16; CHANNELS as usize],
}

struct Out {
    bytes: [u8; 256],
    len: usize,
}

pub struct Board {
    address: Option<u8>,
    input: [u8; 512],
    input_len: usize,
    output: [u8; 1024],
    output_len: usize,
    output_read: usize,
    last_reply: [u8; 512],
    last_reply_len: usize,
    coins: [u16; COIN_SLOTS],
    /// Commands the board did not understand, for the hook's log.
    pub unknown: Option<u8>,
}

impl Board {
    pub const fn new() -> Self {
        Self {
            address: None,
            input: [0; 512],
            input_len: 0,
            output: [0; 1024],
            output_len: 0,
            output_read: 0,
            last_reply: [0; 512],
            last_reply_len: 0,
            coins: [0; COIN_SLOTS],
            unknown: None,
        }
    }

    /// True once the game has given the board an address (the sense line).
    pub fn addressed(&self) -> bool {
        self.address.is_some()
    }

    pub fn insert_coin(&mut self, slot: usize) {
        if let Some(c) = self.coins.get_mut(slot) {
            *c = (*c + 1).min(0x3fff);
        }
    }

    /// Bytes the game wrote. `read_inputs` is called only when a request
    /// needs the current controls.
    pub fn write(&mut self, bytes: &[u8], read_inputs: &mut dyn FnMut() -> Inputs) {
        for &b in bytes {
            if b == SYNC {
                self.input_len = 0;
            }
            if self.input_len < self.input.len() {
                self.input[self.input_len] = b;
                self.input_len += 1;
            }
            if let Some(packet) = self.complete() {
                self.input_len = 0;
                self.handle(&packet.0[..packet.1], read_inputs);
            }
        }
    }

    /// Reply bytes for the game; returns how many were copied.
    pub fn read(&mut self, out: &mut [u8]) -> usize {
        let n = (self.output_len - self.output_read).min(out.len());
        out[..n].copy_from_slice(&self.output[self.output_read..self.output_read + n]);
        self.output_read += n;
        if self.output_read == self.output_len {
            self.output_read = 0;
            self.output_len = 0;
        }
        n
    }

    pub fn pending(&self) -> usize {
        self.output_len - self.output_read
    }

    /// The unescaped packet (destination, length, payload, checksum) once
    /// all of its bytes have arrived.
    fn complete(&self) -> Option<([u8; 260], usize)> {
        if self.input_len < 3 || self.input[0] != SYNC {
            return None;
        }
        let mut packet = [0u8; 260];
        let mut n = 0;
        let mut escaped = false;
        for &b in &self.input[1..self.input_len] {
            if escaped {
                packet[n] = b.wrapping_add(1);
                n += 1;
                escaped = false;
            } else if b == MARK {
                escaped = true;
            } else {
                packet[n] = b;
                n += 1;
            }
            if n == packet.len() {
                return None;
            }
            if n >= 2 && n == 2 + usize::from(packet[1]) {
                return Some((packet, n));
            }
        }
        None
    }

    fn handle(&mut self, packet: &[u8], read_inputs: &mut dyn FnMut() -> Inputs) {
        let (destination, body, sum) = (packet[0], &packet[2..packet.len() - 1], packet[packet.len() - 1]);
        if destination != BROADCAST && Some(destination) != self.address {
            return;
        }
        let checksum = packet[..packet.len() - 1]
            .iter()
            .fold(0u8, |a, &b| a.wrapping_add(b));
        if checksum != sum {
            self.reply(STATUS_CHECKSUM, &[]);
            return;
        }
        // Reset (F0 D9) is never answered.
        if body.first() == Some(&0xf0) {
            if body.get(1) == Some(&0xd9) {
                self.address = None;
            }
            return;
        }
        let mut out = Out { bytes: [0; 256], len: 0 };
        let put = |out: &mut Out, bytes: &[u8]| {
            for &b in bytes {
                if out.len < out.bytes.len() {
                    out.bytes[out.len] = b;
                    out.len += 1;
                }
            }
        };
        let mut at = 0;
        let mut inputs = None;
        while at < body.len() {
            let command = body[at];
            let arg = |i: usize| body.get(at + i).copied().unwrap_or(0);
            let used = match command {
                0xf1 => {
                    // Set address; only unaddressed boards take one.
                    if self.address.is_none() {
                        self.address = Some(arg(1));
                    }
                    put(&mut out, &[REPORT_OK]);
                    2
                }
                0x2f => {
                    // Retransmit: the previous reply, unchanged.
                    let (last, len) = (self.last_reply, self.last_reply_len);
                    self.send(&last[..len]);
                    return;
                }
                0x10 => {
                    put(&mut out, &[REPORT_OK]);
                    put(&mut out, IDENTITY);
                    1
                }
                // Command format revision 1.3, JVS 3.0, communication 1.0.
                0x11 => {
                    put(&mut out, &[REPORT_OK, 0x13]);
                    1
                }
                0x12 => {
                    put(&mut out, &[REPORT_OK, 0x30]);
                    1
                }
                0x13 => {
                    put(&mut out, &[REPORT_OK, 0x10]);
                    1
                }
                0x14 => {
                    put(&mut out, &[REPORT_OK]);
                    put(&mut out, &[0x01, PLAYERS, SWITCHES, 0]);
                    put(&mut out, &[0x02, COIN_SLOTS as u8, 0, 0]);
                    put(&mut out, &[0x03, CHANNELS, ANALOG_BITS, 0]);
                    put(&mut out, &[0x12, 20, 0, 0]);
                    put(&mut out, &[0x00]);
                    1
                }
                0x15 => {
                    // Main board identification: a NUL-terminated string.
                    let end = body[at + 1..]
                        .iter()
                        .position(|&b| b == 0)
                        .map_or(body.len() - at, |p| p + 2);
                    put(&mut out, &[REPORT_OK]);
                    end
                }
                0x20 => {
                    let i = *inputs.get_or_insert_with(&mut *read_inputs);
                    let (players, bytes) = (usize::from(arg(1)), usize::from(arg(2)));
                    put(&mut out, &[REPORT_OK, i.switches[0]]);
                    for player in 0..players {
                        for byte in 0..bytes {
                            put(&mut out, &[if player < 2 && byte < 2 {
                                i.switches[1 + player * 2 + byte]
                            } else {
                                0
                            }]);
                        }
                    }
                    3
                }
                0x21 => {
                    put(&mut out, &[REPORT_OK]);
                    for slot in 0..usize::from(arg(1)) {
                        let c = self.coins.get(slot).copied().unwrap_or(0);
                        put(&mut out, &c.to_be_bytes());
                    }
                    2
                }
                0x22 => {
                    let i = *inputs.get_or_insert_with(&mut *read_inputs);
                    put(&mut out, &[REPORT_OK]);
                    for channel in 0..usize::from(arg(1)) {
                        put(&mut out, &i.analog.get(channel).copied().unwrap_or(0).to_be_bytes());
                    }
                    2
                }
                0x26 => {
                    // Miscellaneous switches: none.
                    put(&mut out, &[REPORT_OK]);
                    for _ in 0..arg(1) {
                        put(&mut out, &[0]);
                    }
                    2
                }
                0x2e => {
                    // Payout remaining: no hopper.
                    put(&mut out, &[REPORT_OK, 0, 0, 0, 0]);
                    2
                }
                0x30 | 0x31 => {
                    // Decrease / increase coins in a slot.
                    let slot = usize::from(arg(1)).wrapping_sub(1);
                    let amount = u16::from_be_bytes([arg(2), arg(3)]);
                    if let Some(c) = self.coins.get_mut(slot) {
                        *c = if command == 0x30 {
                            c.saturating_sub(amount)
                        } else {
                            (*c).saturating_add(amount).min(0x3fff)
                        };
                    }
                    put(&mut out, &[REPORT_OK]);
                    4
                }
                0x32 => {
                    put(&mut out, &[REPORT_OK]);
                    2 + usize::from(arg(1))
                }
                0x33 => {
                    put(&mut out, &[REPORT_OK]);
                    2 + 2 * usize::from(arg(1))
                }
                0x34 => {
                    put(&mut out, &[REPORT_OK]);
                    2 + usize::from(arg(1))
                }
                0x37 | 0x38 => {
                    put(&mut out, &[REPORT_OK]);
                    3
                }
                // Namco's serial pass-through to a cabinet peripheral
                // (open `16 a b`, send `05 n data`, receive `15 a b`). No
                // peripheral is attached: sends are accepted, nothing is
                // ever received.
                0x70 if arg(1) == 0x16 => {
                    put(&mut out, &[REPORT_OK]);
                    4
                }
                0x70 if arg(1) == 0x05 => {
                    put(&mut out, &[REPORT_OK, 1]);
                    3 + usize::from(arg(2))
                }
                0x70 if arg(1) == 0x15 => {
                    put(&mut out, &[REPORT_OK, 0]);
                    4
                }
                _ => {
                    // Unknown: answer what came before and stop here.
                    self.unknown = Some(command);
                    if out.len == 0 {
                        self.reply(STATUS_UNKNOWN, &[]);
                        return;
                    }
                    put(&mut out, &[REPORT_PARAMETER]);
                    body.len() - at
                }
            };
            at += used;
        }
        self.reply(STATUS_OK, &out.bytes[..out.len]);
    }

    fn reply(&mut self, status: u8, reports: &[u8]) {
        let mut packet = [0u8; 260];
        packet[0] = 0; // to the master
        packet[1] = (reports.len() + 2) as u8;
        packet[2] = status;
        packet[3..3 + reports.len()].copy_from_slice(reports);
        let n = 3 + reports.len();
        packet[n] = packet[..n].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        self.last_reply[..n + 1].copy_from_slice(&packet[..n + 1]);
        self.last_reply_len = n + 1;
        self.send(&packet[..n + 1]);
    }

    /// Frame and escape an unescaped packet into the output queue.
    fn send(&mut self, packet: &[u8]) {
        let push = |b: u8, out: &mut Self| {
            if out.output_len < out.output.len() {
                out.output[out.output_len] = b;
                out.output_len += 1;
            }
        };
        push(SYNC, self);
        for &b in packet {
            if b == SYNC || b == MARK {
                push(MARK, self);
                push(b - 1, self);
            } else {
                push(b, self);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(destination: u8, body: &[u8]) -> Vec<u8> {
        let mut raw = vec![destination, body.len() as u8 + 1];
        raw.extend_from_slice(body);
        raw.push(raw.iter().fold(0u8, |a, &b| a.wrapping_add(b)));
        let mut out = vec![SYNC];
        for b in raw {
            if b == SYNC || b == MARK {
                out.extend([MARK, b - 1]);
            } else {
                out.push(b);
            }
        }
        out
    }

    fn exchange(board: &mut Board, destination: u8, body: &[u8], inputs: Inputs) -> Vec<u8> {
        board.write(&request(destination, body), &mut || inputs);
        let mut out = [0u8; 1024];
        let n = board.read(&mut out);
        // Unescape and strip sync, destination, length and checksum.
        let mut plain = Vec::new();
        let mut escaped = false;
        for &b in &out[1..n] {
            if escaped {
                plain.push(b + 1);
                escaped = false;
            } else if b == MARK {
                escaped = true;
            } else {
                plain.push(b);
            }
        }
        assert_eq!(plain[0], 0, "replies go to the master");
        assert_eq!(usize::from(plain[1]), plain.len() - 2);
        let sum = plain[..plain.len() - 1].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        assert_eq!(sum, *plain.last().unwrap());
        plain[2..plain.len() - 1].to_vec()
    }

    #[test]
    fn reset_and_addressing_follow_the_sense_line() {
        let mut b = Board::new();
        b.write(&request(BROADCAST, &[0xf0, 0xd9]), &mut Inputs::default);
        assert_eq!(b.pending(), 0, "reset is never answered");
        assert!(!b.addressed());
        assert_eq!(exchange(&mut b, BROADCAST, &[0xf1, 1], Inputs::default()), [1, 1]);
        assert!(b.addressed());
        // Packets for other nodes are ignored.
        b.write(&request(2, &[0x10]), &mut Inputs::default);
        assert_eq!(b.pending(), 0);
        b.write(&request(BROADCAST, &[0xf0, 0xd9]), &mut Inputs::default);
        assert!(!b.addressed());
    }

    #[test]
    fn identity_features_and_inputs() {
        let mut b = Board::new();
        exchange(&mut b, BROADCAST, &[0xf1, 1], Inputs::default());
        let id = exchange(&mut b, 1, &[0x10], Inputs::default());
        assert_eq!(id[..2], [STATUS_OK, REPORT_OK]);
        assert_eq!(&id[2..], IDENTITY);
        let features = exchange(&mut b, 1, &[0x14], Inputs::default());
        assert_eq!(features[2..6], [1, PLAYERS, SWITCHES, 0]);
        assert_eq!(*features.last().unwrap(), 0);

        let mut inputs = Inputs::default();
        inputs.switches = [0x80, 0x02, 0x90, 0, 0];
        inputs.analog[0] = 0xe0d0; // both bytes need escaping
        inputs.analog[1] = 0x1234;
        b.insert_coin(0);
        let reply = exchange(
            &mut b,
            1,
            &[0x20, 2, 2, 0x21, 2, 0x22, 3],
            inputs,
        );
        assert_eq!(
            reply,
            [
                STATUS_OK, 1, 0x80, 0x02, 0x90, 0, 0, // switches
                1, 0, 1, 0, 0, // coins
                1, 0xe0, 0xd0, 0x12, 0x34, 0, 0 // analog
            ]
        );
        // Coins are spent by the game.
        assert_eq!(exchange(&mut b, 1, &[0x30, 1, 0, 1, 0x21, 1], inputs), [1, 1, 1, 0, 0]);
    }

    #[test]
    fn outputs_are_acknowledged_and_unknown_commands_reported() {
        let mut b = Board::new();
        exchange(&mut b, BROADCAST, &[0xf1, 1], Inputs::default());
        assert_eq!(
            exchange(&mut b, 1, &[0x32, 3, 1, 2, 3, 0x33, 1, 0, 0], Inputs::default()),
            [1, 1, 1]
        );
        // The serial pass-through as the game polls it each frame.
        assert_eq!(
            exchange(&mut b, 1, &[0x70, 0x16, 0x20, 0x06], Inputs::default()),
            [1, 1]
        );
        assert_eq!(
            exchange(
                &mut b,
                1,
                &[0x21, 1, 0x70, 0x05, 2, 0xaa, 0xbb, 0x70, 0x15, 0, 0xff],
                Inputs::default()
            ),
            [1, 1, 0, 0, 1, 1, 1, 0]
        );
        assert_eq!(exchange(&mut b, 1, &[0x99], Inputs::default()), [STATUS_UNKNOWN]);
        assert_eq!(b.unknown, Some(0x99));
        // A bad checksum is reported, not executed.
        b.write(&[SYNC, 1, 2, 0x10, 0x55], &mut Inputs::default);
        let mut out = [0u8; 16];
        let n = b.read(&mut out);
        assert_eq!(out[..n], [SYNC, 0, 2, STATUS_CHECKSUM, 5]);
    }
}
