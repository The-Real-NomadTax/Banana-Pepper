//! Measured eight-byte input report consumed by Ford EU 1.1.6 at 0x4a1af0.
//! Kept dependency-free so native Windows and Wine use the same adapter.
pub struct Inputs {
    pub steering: u8,
    pub gas: u8,
    pub brake: u8,
    pub start: bool,
    pub view: bool,
    pub shift_up: bool,
    pub shift_down: bool,
}
impl Inputs {
    pub fn report(&self) -> [u8; 8] {
        [
            0,
            self.steering,
            0,
            self.gas,
            0,
            self.brake,
            (if self.start { 4 } else { 0 })
                | (if self.view { 8 } else { 0 })
                | (if self.shift_up { 1 } else { 0 })
                | (if self.shift_down { 2 } else { 0 }),
            0,
        ]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calibrated_axes_and_cabinet_switches_match_the_guest_reader() {
        let mut i = Inputs {
            steering: 128,
            gas: 255,
            brake: 0,
            start: true,
            view: false,
            shift_up: false,
            shift_down: false,
        };
        assert_eq!(i.report(), [0, 128, 0, 255, 0, 0, 4, 0]);
        i.gas = 0;
        i.brake = 255;
        i.steering = 0;
        i.start = false;
        i.view = true;
        assert_eq!(i.report(), [0, 0, 0, 0, 0, 255, 8, 0]);
    }
}
