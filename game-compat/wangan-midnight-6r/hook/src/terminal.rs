//! The cabinet link's wire format and the terminal's broadcast.
//!
//! Cabinets and the Wangan terminal exchange UDP datagrams on port 50765:
//! a 4-byte header (flags, sender node, little-endian length of header and
//! payload), a `wm.protobuf.Message` (the schema is embedded in the game),
//! and a big-endian CRC-32 of everything before it (MSB first, polynomial
//! 0x04C11DB7, initial value 0xFFFFFFFF, no final XOR). The format was
//! read from the drive cabinet's own heartbeat; the test below rebuilds one
//! byte for byte.
//!
//! No `std` use, so the format is unit-tested on the host.

/// A protobuf writer over a fixed buffer.
pub struct Writer<'a> {
    buffer: &'a mut [u8],
    len: usize,
}

impl<'a> Writer<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self { buffer, len: 0 }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn bytes(&self) -> &[u8] {
        &self.buffer[..self.len]
    }
    fn byte(&mut self, b: u8) {
        if self.len < self.buffer.len() {
            self.buffer[self.len] = b;
            self.len += 1;
        }
    }
    fn varint(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.byte(v as u8 | 0x80);
            v >>= 7;
        }
        self.byte(v as u8);
    }
    /// A varint field (int32, uint32, bool, enum).
    pub fn int(&mut self, field: u32, value: i64) -> &mut Self {
        self.varint(u64::from(field) << 3);
        self.varint(value as u64);
        self
    }
    /// A zigzag-encoded sint32 field.
    pub fn sint(&mut self, field: u32, value: i32) -> &mut Self {
        self.varint(u64::from(field) << 3);
        self.varint(((value << 1) ^ (value >> 31)) as u32 as u64);
        self
    }
    /// A length-delimited field (string, bytes, nested message).
    pub fn bytes_field(&mut self, field: u32, value: &[u8]) -> &mut Self {
        self.varint((u64::from(field) << 3) | 2);
        self.varint(value.len() as u64);
        for &b in value {
            self.byte(b);
        }
        self
    }
}

/// CRC-32, MSB first, polynomial 0x04C11DB7, initial value 0xFFFFFFFF.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Frame `message` from `node` into `out`; returns the datagram length.
pub fn frame(flags: u8, node: u8, message: &[u8], out: &mut [u8]) -> usize {
    let body = 4 + message.len();
    if body + 4 > out.len() || body > usize::from(u16::MAX) {
        return 0;
    }
    out[0] = flags;
    out[1] = node;
    out[2..4].copy_from_slice(&(body as u16).to_le_bytes());
    out[4..body].copy_from_slice(message);
    let crc = crc32(&out[..body]);
    out[body..body + 4].copy_from_slice(&crc.to_be_bytes());
    body + 4
}

/// The operator settings a terminal distributes to its cabinets
/// (`wm.protobuf.BatchSetting`).
#[derive(Clone, Copy)]
pub struct Settings {
    pub coin_chute: i64,
    pub buycard_cost: i64,
    pub game_cost: i64,
    pub continue_cost: i64,
    pub fullcourse_cost: i64,
    pub freeplay: bool,
    pub wins_and_remains: i64,
    pub time_zone_type: i64,
}

impl Settings {
    /// The cabinet build's defaults, with free play.
    pub const DEFAULT: Self = Self {
        coin_chute: 1,
        buycard_cost: 3,
        game_cost: 2,
        continue_cost: 2,
        fullcourse_cost: 4,
        freeplay: true,
        wins_and_remains: 1,
        time_zone_type: 36,
    };
}

/// What the terminal reports about itself besides the operator settings.
#[derive(Clone, Copy)]
pub struct Terminal<'a> {
    pub frame_number: u32,
    pub serial: &'a [u8],
    /// The software revision; a cabinet whose own revision differs reports
    /// a version mismatch.
    pub revision: i32,
    /// The terminal's clock (Unix seconds); a cabinet waits for a non-zero
    /// value before leaving its terminal wait.
    pub sync_time: i32,
}

/// The terminal's periodic broadcast (`wm.protobuf.Message`): its heartbeat,
/// its settings state, the batch settings and the system information.
pub fn terminal_message(terminal: &Terminal, settings: &Settings, out: &mut [u8]) -> usize {
    let mut heart = [0u8; 48];
    let mut h = Writer::new(&mut heart);
    h.int(1, terminal.frame_number.into()).bytes_field(2, terminal.serial).int(3, 0);
    let mut batch = [0u8; 128];
    let mut b = Writer::new(&mut batch);
    b.int(1, settings.coin_chute)
        .int(2, settings.buycard_cost)
        .int(3, settings.game_cost)
        .int(4, settings.continue_cost)
        .int(5, settings.fullcourse_cost)
        .int(6, settings.freeplay.into())
        .int(7, settings.wins_and_remains)
        .int(8, 0)
        .int(9, 0)
        .int(19, terminal.revision.into())
        .int(20, terminal.sync_time.into())
        .int(22, settings.time_zone_type);
    let mut info = [0u8; 16];
    let mut i = Writer::new(&mut info);
    i.int(1, 1).sint(2, 0);
    // `batch_setting_state` (`State`: 1 checking, 2 NG, 3 OK): drive
    // cabinets wait while their terminal reports 1.
    let mut m = Writer::new(out);
    m.bytes_field(5, h.bytes())
        .int(6, 3)
        .bytes_field(8, b.bytes())
        .bytes_field(9, i.bytes());
    m.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A heartbeat the drive cabinet sent (serial 280803400001, frame 0x2c5).
    const CABINET: &str = "0000430012 0e0a00100418002000280030003800 1a025a00 2a1308c505120c323830383033343030303031 1800 3003 4a06080110011800 5a0408001000 6204080012 00d45ef7e6";

    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
        digits
            .chunks(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn rebuilds_the_cabinet_heartbeat() {
        let packet = hex(CABINET);
        let mut heart = [0u8; 32];
        let mut h = Writer::new(&mut heart);
        h.int(1, 0x2c5).bytes_field(2, b"280803400001").int(3, 0);
        let mut m = [0u8; 128];
        let mut w = Writer::new(&mut m);
        w.bytes_field(2, &hex("0a00100418002000280030003800"))
            .bytes_field(3, &hex("5a00"))
            .bytes_field(5, h.bytes())
            .int(6, 3)
            .bytes_field(9, &hex("080110011800"))
            .bytes_field(11, &hex("08001000"))
            .bytes_field(12, &hex("08001200"));
        let mut out = [0u8; 160];
        let n = frame(0, 0, w.bytes(), &mut out);
        assert_eq!(&out[..n], &packet[..]);
    }

    #[test]
    fn terminal_message_is_framed_and_checksummed() {
        let mut message = [0u8; 160];
        let terminal = Terminal {
            frame_number: 7,
            serial: b"280801400002",
            revision: 1,
            sync_time: 1_700_000_000,
        };
        let len = terminal_message(&terminal, &Settings::DEFAULT, &mut message);
        let mut out = [0u8; 200];
        let n = frame(0, 1, &message[..len], &mut out);
        assert_eq!(n, len + 8);
        assert_eq!(usize::from(u16::from_le_bytes([out[2], out[3]])), len + 4);
        assert_eq!(crc32(&out[..n - 4]).to_be_bytes(), out[n - 4..n]);
        // Heartbeat first: field 5, then frame number 7 and the serial.
        assert_eq!(out[4], 0x2a);
        assert_eq!(&out[6..8], &[0x08, 7]);
        assert_eq!(&out[10..22], b"280801400002");
        assert_eq!(sint_round_trip(-1), 1);
    }

    fn sint_round_trip(v: i32) -> u8 {
        let mut b = [0u8; 4];
        let mut w = Writer::new(&mut b);
        w.sint(1, v);
        b[1]
    }
}
